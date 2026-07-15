# 0053. Split a shared hunk into per-symbol sub-hunks at attribution time

- Status: accepted
- Date: 2026-07-15

## Context

ADR 0029 changed the diff pane's file-selection attribution rule from
"first symbol only" to "every symbol whose range the hunk intersects",
cloning the whole hunk into each owning section. That ADR's own
Alternatives section already named and rejected the finer-grained
option — "split a multi-symbol hunk into per-symbol sub-hunks at
attribution time" — as "substantially more complex" for the problem it
was solving (auto-scroll silently doing nothing), while flagging it
could be "revisited if reviewers find the duplicated-hunk-body a real
annoyance in practice."

That revisit condition has now been met. Real review sessions confirm
that walking the left-pane symbol list on a file where several
adjacent symbols share a hunk (a brand-new file being the guaranteed
case, per ADR 0029's own Context) shows the *same hunk body, verbatim,
repeatedly* as the reviewer moves from symbol to symbol — the visible
trade ADR 0029 accepted turns out to cost more reading effort than
expected in practice. This ADR replaces whole-hunk duplication with
per-symbol splitting, amending ADR 0029's attribution step (not its
core "attribute to every intersecting symbol" rule, which stays).

## Decision

**Split only when a hunk intersects two or more symbol ranges.** A
hunk touching zero or one symbol is attributed unchanged (the existing
early-return path) — no new sub-hunk machinery runs for the common
case.

**Row ownership resolves in a single pass over the hunk's lines.**
Added/Context lines carry a new-file line number (the same derivation
`ui::diff_pane::new_side_line_numbers` already uses), so each is
attributed to whichever symbol range contains that line number, or to
"no symbol" when none does. Removed lines carry no new-file line
number of their own — a removal is a *position*, not a range
(`diff_view::Hunk::new_range`'s own zero-width convention for a pure
deletion) — so a Removed line inherits the owner of the current run:
the most recently resolved Added/Context line's owner, or, when a hunk
opens with Removed lines before any Added/Context line has appeared,
the owner of the *next* Added/Context line the scan reaches
(retroactive attribution, resolved by buffering the leading Removed
run until an owner is known).

A maximal run of consecutive lines sharing the same owner (including
the "no symbol" owner) becomes one sub-hunk; the owner changing —
including transitions into or out of "no symbol" — starts a new
sub-hunk.

**An unowned run becomes its own sub-hunk, routed to the module-level
bucket.** Import lines and the gaps between symbols are not
absorbed into whichever symbol happens to be nearest; they keep the
existing `MODULE_LEVEL_TITLE` treatment, just as a sub-hunk instead of
a share of a larger hunk.

**A line owned by more than one symbol still appears in every owning
symbol's sub-hunk.** Symbol ranges are expected to be non-overlapping
in practice (a real extractor's contract), so "one line, one owner"
covers the common case and is what makes splitting eliminate
duplication there. When ranges do overlap — pathological input, not a
real extractor's output — ADR 0029's "attribute to every intersecting
symbol" rule still governs: the line-ownership scan is repeated once
per distinct owner rather than picking a single winner, so an
overlapping pair of symbols still both receive that line, same as
before this ADR. This is the one shape a split does not shrink the
duplication for, because the duplication there reflects a genuine
double ownership, not an artifact of whole-hunk attribution.

**No context padding at sub-hunk boundaries.** A sub-hunk holds exactly
the contiguous run of lines resolved to its owner — no extra lines are
duplicated onto an adjacent sub-hunk for readability. Padding would
reintroduce the cross-section duplication this ADR exists to remove,
just at a smaller scale.

**Every sub-hunk gets an exactly recomputed `@@` header,** not an
approximation:

- New-side start/count come directly from the sub-hunk's own resolved
  line run (mirroring `diff_view::Hunk::new_range`'s existing
  start/count convention).
- Old-side start is the original hunk's old-side start plus the number
  of old-side lines (Removed + Context) consumed by every sub-hunk that
  precedes this one within the same original hunk — computed by
  running total during the same single-pass scan, not derived
  separately afterward.

**New module `rinkaku-tui/src/hunk_split.rs`**, following the
`split_pairing.rs` precedent (also split out of `diff_shape.rs` for an
independent responsibility) rather than growing `diff_shape.rs`
further. `build_file_content` calls into it and no longer performs
attribution inline. Tests are large enough to warrant the same
`#[cfg(test)] #[path = "hunk_split_tests/..."] mod tests;` split
`diff_shape_tests`/`split_pairing_tests` already use.

**Highlight lookup needs one new field, not a new lookup mechanism.**
`highlight::highlight_hunk` still computes one `Vec<LineHighlight>` per
*original* hunk, keyed by `source_index` exactly as before — re-running
highlighting per sub-hunk would be both wasteful (highlighting is the
expensive step per that module's own doc comment) and pointless (the
highlight data is the same tokens, just sliced differently). Instead
`AttributedHunk` gains `origin_offset: usize`, the sub-hunk's start
index within the original hunk's `lines`. The render side
(`ui::diff_pane::diff_pane_lines`/`diff_pane_split_rows`) already
indexes a hunk's own `lines` positionally when looking up its
highlight; it now offsets that index by `origin_offset` before
indexing into the `source_index`-keyed highlight slice — the same
"index-based indirection instead of a new lookup" shape
`split_pairing::SplitRow::left_index`/`right_index` already
established for exactly this kind of problem (rows that don't
positionically line up with the original `lines` slice one-to-one).

**`]c`/`[c` and auto-scroll need no code change.** `hunk_start_lines`,
`walk_sections`, `section_start_line_for_symbol`, and
`symbol_id_for_scroll_line` only count hunks that are actually present
in a section's `hunks: Vec<AttributedHunk>` — they have no opinion on
where those `AttributedHunk`s came from. Once `build_file_content`
stops duplicating a shared hunk and instead emits one (smaller)
sub-hunk per owning section, these functions' existing per-section walk
produces exactly one jump stop per rendered sub-hunk automatically. Only
their test fixtures' expected values change, not their implementation.

## Alternatives

- **Keep whole-hunk duplication (status quo, ADR 0029).** Rejected:
  this is the exact problem prompting this ADR — dogfooding confirms
  the duplicated hunk body is a real reading-effort cost, not a
  theoretical one, and ADR 0029 itself named this as the condition
  under which it should be revisited.
- **Nearest-symbol attribution for unowned (module-level) runs**,
  folding import/gap lines into whichever symbol section is closest
  rather than a separate module-level bucket. Rejected: this would
  misattribute lines to a symbol that never actually changed them,
  undermining the diff pane's per-symbol "orient before reading"
  purpose (ADR 0020) that this whole attribution system exists to
  serve — an import line under a function's header looks like part of
  that function's diff when it is not.
- **Pad sub-hunk boundaries with a few lines of shared context** so a
  reader jumping to one symbol's section sees a line or two of the
  neighboring change for orientation. Rejected: this reintroduces the
  duplication this ADR removes, just bounded to N lines instead of the
  whole hunk — the annoyance driving this ADR was specifically about
  repeated content, and a small amount of repeated content is still
  repeated content.
- **Approximate the old-side header** (e.g. reuse the original hunk's
  old-side start for every sub-hunk, or split old-side count evenly).
  Rejected: an inaccurate `@@` header is actively misleading if a
  reviewer ever compares it against the real diff (e.g. copy-pasting it
  into another tool, or cross-checking against `git diff` output side
  by side) and the accurate value is no harder to compute — it is a
  running total already available during the same single pass that
  resolves line ownership.
- **Implement splitting inline in `diff_shape.rs`.** Rejected on
  CLAUDE.md's file-size discipline: `diff_shape.rs` is already 417
  lines covering section-building and unified-view line counting;
  adding a second, independent responsibility (line-level hunk
  splitting) to the same file repeats the exact situation
  `split_pairing.rs` was already extracted from once. A new
  responsibility gets a new module, per `split_pairing.rs`'s own
  precedent.

## Consequences

- The diff pane no longer shows a shared hunk's body verbatim under
  every owning symbol's section — each section shows only the lines
  that are actually "its own", so scrolling through a file's sections
  no longer re-reads the same text repeatedly.
- `hunk_start_lines`'s "`]c`/`[c` has one stop per rendered hunk
  occurrence" behavior (ADR 0029's own framing) is preserved exactly,
  just now against smaller, non-duplicated sub-hunks instead of
  repeated whole hunks — a reviewer jumping hunk-to-hunk sees each
  stop's content exactly once across the file rather than once per
  owning section.
- `AttributedHunk` gains an `origin_offset: usize` field — every
  existing test fixture that constructs an `AttributedHunk` directly
  (`diff_shape_tests`, `diff_pane_tests`, `source_screen_tests`, and
  any other call site) needs that field threaded through. For a hunk
  that was never split (the common case, single-owner hunks),
  `origin_offset` is always `0`.
- ADR 0029's own two regression tests
  (`should_attribute_overlapping_hunk_to_every_symbol_it_intersects`,
  `should_attribute_new_file_single_hunk_to_every_symbol_it_defines`)
  change their expected output from "the same whole hunk cloned into
  every owning section" to "each section receiving its own, smaller
  sub-hunk" — this amends ADR 0029's attribution step; ADR 0029's core
  decision (attribute to every intersecting symbol, not just the
  first) is unchanged and still the reason a new-file symbol's
  auto-scroll target resolves at all.
- No change to `rinkaku-core` or to Markdown/JSON output —
  `hunk_split.rs` lives entirely in `rinkaku-tui`'s presentation layer,
  same scope boundary ADR 0029 already confirmed for this attribution
  step.
- No backward-compatibility concern: the TUI has not shipped a release
  (ADR 0015/0016, restated by ADR 0020, ADR 0027, and ADR 0029), so
  this amendment applies in place.
