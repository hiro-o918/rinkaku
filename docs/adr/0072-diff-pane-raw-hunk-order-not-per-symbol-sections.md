# 0072. Diff pane shows raw hunks in original order, not per-symbol sections

- Status: accepted (scroll-sync granularity amended by ADR 0074)
- Date: 2026-08-11

## Context

ADR 0020 decision 4 introduced the diff pane's per-symbol section
grouping: a file selection's hunks are split (or, at that point, cloned)
under one heading per symbol, with any hunk touching no symbol's line
range collected into a trailing `"(module level)"` bucket. ADR 0027
extended this shape to symbol-row selections too (always render the whole
file, auto-scroll to the selected symbol's section) and ADR 0029/0053
progressively refined how a hunk shared by more than one symbol is
attributed and split.

Sustained dogfooding across several PRs surfaces the accumulated cost of
this reshaping, not just its individual refinements:

- **Context-only fragments read as noise.** A hunk (or ADR 0053 sub-hunk)
  that intersects no symbol range — an import block, a blank line between
  two functions, a doc comment attached loosely enough that the extractor
  does not fold it into the following symbol's range — lands in the
  `"(module level)"` bucket at the very end of the file, detached from
  the symbols it sits next to in the real file. A reviewer scanning
  top-to-bottom the way they would read `git diff` output instead has to
  jump to the end of the pane to find a change that, in the actual file,
  sits three lines above the symbol they were just reading.
- **Decorator/attribute lines can be severed from the definition they
  annotate.** ADR 0053's line-ownership scan resolves each line's owner
  independently by new-side position; a decorator or attribute line
  immediately above a changed signature does not always resolve to the
  same owner as the signature itself (its own line falls just outside the
  extractor's recorded range), so the two end up in different sections —
  or one in a symbol's section and the other in the module-level bucket —
  even though they read as one inseparable edit in the real diff.
- **The reshaping is a lossy rebuild of something the reviewer already
  has.** `git diff`/`gh pr diff` already presents hunks in file order,
  which is the order a reviewer's mental model of the file expects. Every
  ADR since 0020 has been chasing correctness for a transformation
  (attribute lines to symbols, split shared hunks, avoid duplication)
  whose entire purpose was to recover an ordering and grouping the input
  already provided for free.

These are not independent bugs to patch one at a time — ADR 0029 already
had to amend ADR 0020's attribution rule once, and ADR 0053 amended it
again for the duplication that fix introduced. Continuing to refine
attribution is chasing the symptom; the reshaping itself is the thing
producing the symptoms.

## Decision

**The diff pane renders a selected file's hunks in their original
`git diff` order and content — no symbol-section grouping, no per-symbol
splitting, no module-level bucket.** Selecting a file or a symbol within
it shows the same content: the whole file's hunks, exactly as `diff_view`
parsed them.

**`DiffPaneContent` narrows to a flat hunk list.** `DiffSection`,
`ContractHeader`, and `MODULE_LEVEL_TITLE` are deleted.
`DiffPaneContent::File` now wraps `Vec<AttributedHunk>` instead of
`Vec<DiffSection>`; `AttributedHunk` keeps only `source_index` and `hunk`
(its `origin_offset` field is deleted along with the sub-hunk splitting
that field existed to support). `crate::hunk_split` is deleted outright —
ADR 0053's whole reason to exist (avoid re-showing a shared hunk once per
owning section) has no object once there are no sections to duplicate a
hunk across.

**Symbol selection auto-scrolls to the first hunk intersecting that
symbol's range**, using `diff_view::hunk_intersects` (the same half-open
rule `hunks_for_range` already applies) directly against the file's flat
hunk list instead of a precomputed section start. A symbol whose range
intersects no hunk (the same "no hunks of its own" case ADR 0020's
now-deleted `sections.retain` used to silently drop) falls back to
scrolling to the top of the file — the file is fully shown either way, so
"nothing to scroll to" is not a dead end, just an uninteresting no-op
scroll.

**The reverse direction (`crate::diff_shape::symbol_id_for_scroll_line`,
ADR 0030) resolves a scroll position to a symbol by whole-hunk
intersection, not by section membership.** Given the scroll line, find
which hunk's rendered span (header row through its last body row,
inclusive — `hunk_start_lines`' boundary table) it falls inside, then
return the first symbol (source order) whose `LineRange` intersects that
hunk via `hunk_intersects`' existing half-open rule — the same
"first-intersecting" pairing `section_start_line_for_symbol` uses in the
opposite direction, so a hunk owned by exactly one symbol resolves
consistently both ways. Whole-hunk intersection rather than a per-line
new-side-number lookup is deliberate: the scroll target a symbol
selection lands on is a hunk's *header* row (no line number of its own),
so a per-line lookup would fail to resolve the exact row auto-scroll just
placed the reviewer on — the regression this ADR's own dynamic
verification caught in `scroll_sync_wrap_tests.rs`. A scroll position
inside a hunk that intersects no symbol resolves to `None`, same as
before — ADR 0030's own "leave the tree cursor untouched rather than
guess" rule for this case is unchanged, it is only computed a different
way.

**`]c`/`[c` walk plain hunk boundaries.** `hunk_start_lines` becomes a
direct one-entry-per-hunk walk over `DiffPaneContent::File`'s flat list —
no section anchor rows, no per-mode (`DiffViewMode`) row-count
divergence to account for, since there is no anchor left to diverge on.
The `view_mode: DiffViewMode` parameter is dropped from
`hunk_start_lines`, `section_start_line_for_symbol`, and
`symbol_id_for_scroll_line` — unified and split render the exact same row
count per hunk (`split_pairing::pair_hunk_lines`'s existing
same-length-as-input invariant already guaranteed this for hunk bodies;
removing the per-section anchor removes the one place the two modes used
to disagree, ADR 0044's "mode-aware row counts for the section anchor"
amendment). `crate::event_loop`'s `effective_diff_view_mode` threading
into `ui::DrawOutcome` stays — it still governs ADR 0044 decision 7's
narrow-terminal split-to-unified fallback — but no longer needs to reach
`diff_shape` at all.

**The pane header's `range:` line now always covers the whole file, not
just the selected symbol.** Before this ADR, a symbol-row selection
scoped `diff_pane_header_lines`' changed-line-ranges summary to that one
symbol's own sections (`App::selected_diff_focus`-filtered), while a
file-row selection covered every section. Since content itself is now
identical for both row kinds (whole file, always), keeping a
narrower-than-shown range summary on a symbol row would misdescribe the
body actually on screen — the header now always summarizes every hunk in
the file, matching what the pane body always renders.

**Unified/split rendering, highlighting, and the `MIN_SPLIT_VIEW_WIDTH`
fallback are unaffected.** `diff_pane_lines`/`diff_pane_split_rows` keep
rendering hunk headers and bodies exactly as before; only the
section-anchor scaffolding (`section_anchor_lines`,
`section_anchor_split_row`, the `show_section_headers` parameter) is
deleted, since there is no longer a section to anchor. Highlighting stays
keyed by `source_index` into `HighlightedFile::hunks` — `origin_offset`
disappears from the lookup because every `AttributedHunk` now
corresponds 1:1, unsliced, to its original hunk (`origin_offset` was
always `0` for that case already).

**Contract-change disclosure is dropped from the diff pane, not from the
app.** ADR 0020 decision 4's 2-line old/new signature header
(`ContractHeader`) is deleted along with the sections it used to prefix.
The Detail pane (`crate::detail::build_detail`, `SignatureView::Changed`)
already renders the identical old/new signature comparison independently
of the diff pane and is unaffected by this ADR — a reviewer who wants
"what did the contract look like before" still has it one pane away,
just no longer duplicated inline above the hunks.

**Split (side-by-side) view and its row-pairing (`crate::split_pairing`)
are unaffected.** `pair_hunk_lines` operates within one hunk's lines and
has no dependency on section grouping; it continues to run per hunk in
`diff_pane_split_rows` exactly as before this ADR.

## Alternatives

- **Keep per-symbol sections, fix decorator/context-line attribution
  instead.** Rejected: this is exactly the "refine attribution again"
  path ADR 0029 and ADR 0053 already took twice, and the module-level
  bucket's structural problem (unowned content is not *near* the symbols
  it sits next to in the file) is not an attribution bug to fix — it is
  the section model itself disagreeing with the reviewer's file-order
  mental model. A third attribution refinement would still leave that
  disagreement in place.
- **Keep sections for a file selection, drop them only for a symbol
  selection (revert to something closer to ADR 0020's original
  pre-ADR-0027 clip).** Rejected: reintroduces the exact "moving the
  cursor produces no visible motion between adjacent symbols in the same
  file" complaint ADR 0027 was written to fix, and still leaves the
  module-level-bucket/decorator-severing problems unresolved for the file
  view, which is the view every symbol selection was unified into by ADR
  0027 in the first place.
- **Keep `AttributedHunk`'s `origin_offset` field for forward
  compatibility with a future re-introduction of splitting.** Rejected:
  YAGNI — an unused field carried "just in case" is exactly the kind of
  speculative surface CLAUDE.md's shared-abstraction guidance already
  discourages; if a future feature needs sub-hunk slicing again, it can
  reintroduce the field with a concrete requirement driving its shape,
  which may not even match `hunk_split.rs`'s prior shape.
- **Collapse decorator/context lines into the *nearest* symbol instead of
  removing sections outright.** Considered as a smaller fix confined to
  `hunk_split`'s ownership resolution. Rejected for the same reason ADR
  0053's own Alternatives section already rejected nearest-symbol
  attribution: it misattributes lines to a symbol that never actually
  changed them, which is a worse failure mode (silently wrong) than a
  flat hunk-ordered view (nothing is attributed to anything, so nothing
  can be mis-attributed).

## Consequences

- The diff pane's file-selected and symbol-selected views are now
  identical in content (the whole file's hunks, original order) and
  differ only in initial scroll position — collapsing ADR 0027's
  "file-scoped shape, symbol selection is an auto-scroll target" wording
  from "target = section start" to "target = first intersecting hunk's
  start", a narrower change than it sounds because the shape itself
  (`DiffPaneContent::File`) was already unified across both row kinds.
- `crate::hunk_split` (ADR 0053) is deleted in full, along with its test
  module `hunk_split_tests/`. ADR 0053 is superseded by this ADR: the
  duplication problem it solved no longer exists because there is
  nothing left to duplicate a hunk across.
- `DiffSection`, `ContractHeader`, and `MODULE_LEVEL_TITLE` are deleted
  from `crate::diff_shape`. Every test constructing a `DiffSection`
  fixture is rewritten against the flat `AttributedHunk` list; ADR 0020's
  original per-symbol-section screenshots/expectations and ADR 0029's two
  regression tests
  (`should_attribute_overlapping_hunk_to_every_symbol_it_intersects`,
  `should_attribute_new_file_single_hunk_to_every_symbol_it_defines`) are
  removed as no longer applicable — a hunk is shown exactly once,
  regardless of how many symbols its range happens to intersect, so there
  is no "which symbol wins the hunk" question left to regression-test.
- `crate::ui::diff_pane`'s `show_section_headers` parameter,
  `section_anchor_lines`, and `section_anchor_split_row` are deleted.
  `diff_pane_lines`/`diff_pane_split_rows` render a hunk header followed
  directly by its body lines, with a blank separator between hunks (no
  separator role left for a section boundary).
- `crate::diff_shape::hunk_start_lines`, `section_start_line_for_symbol`,
  and `symbol_id_for_scroll_line` drop their `view_mode: DiffViewMode`
  parameter; every call site in `crate::event_loop::scroll_sync` and
  `crate::event_loop::dispatch_non_source_key` stops passing
  `effective_diff_view_mode` into them. `ui::DrawOutcome::effective_diff_view_mode`
  and its narrow-terminal-fallback role (ADR 0044 decision 7) are
  unaffected — that plumbing still exists for `MIN_SPLIT_VIEW_WIDTH`, it
  simply no longer needs to reach `diff_shape`.
- A reviewer reading a file's diff now sees exactly the same hunk
  ordering and content `git diff`/`gh pr diff` would show for that file —
  the diff pane's own "orient before reading" framing (ADR 0020) now
  applies only at the pane-header level (name, badges, changed-line
  ranges) and via the Detail pane's independent contract-change view, not
  via inline per-symbol section headers.
- No change to `rinkaku-core` or to Markdown/JSON output — this ADR, like
  every ADR it supersedes/amends, is scoped entirely to
  `rinkaku-tui`'s presentation layer.
- No backward-compatibility concern: the TUI has never shipped a release
  (ADR 0015/0016, restated by every TUI-scoped ADR since), so this
  replaces the prior semantics in place.

## Amendment history of superseded/amended ADRs

- **Supersedes ADR 0020** decision 4's per-symbol section
  grouping/module-level bucket/contract-header sub-rules (decision 4
  only; ADR 0020's focus model, default-pane, and help-overlay decisions
  are untouched).
- **Supersedes ADR 0053** in full (per-symbol hunk splitting has no
  remaining purpose).
- **Amends ADR 0027**: "always render the whole file" is unchanged and
  restated by this ADR as the *only* shape rather than one of two
  (file-row/symbol-row); "auto-scroll to the selected symbol's section"
  narrows to "auto-scroll to the selected symbol's first intersecting
  hunk". Decisions 4-7 (who owns the auto-scroll, scroll preserved vs.
  reset, `]c`/`[c` walks every hunk) are unaffected.
- **Amends ADR 0029**: the "attribute a hunk to every symbol it
  intersects" rule is moot (there is no per-symbol attribution left to
  perform), but this ADR's finding that a hunk can legitimately intersect
  more than one symbol's range remains true and informs why symbol
  auto-scroll now targets "first intersecting hunk" rather than assuming
  a 1:1 hunk-to-symbol mapping.
- **Amends ADR 0044**: the "mode-aware row counts for the section
  anchor" amendment is moot (no anchor left); decisions 1-8 and the two
  other amendments (default-to-Split, similarity-based alignment) are
  unaffected.
