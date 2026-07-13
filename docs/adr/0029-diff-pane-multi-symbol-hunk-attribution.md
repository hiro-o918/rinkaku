# 0029. Diff pane attributes a hunk to every symbol it intersects, not just the first

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0020 decision 4 established the diff pane's file-selection shape:
group a file's hunks under per-symbol section headers, and when one
hunk intersects more than one symbol's line range, attribute it to
**only the first symbol (source order) it intersects** — rejecting
duplicate attribution because it "would make 'total lines shown' no
longer match 'total lines in the diff', actively misleading about
change size for exactly the file-level view meant to summarize it."
ADR 0027 later made every symbol-row selection auto-scroll into this
same shaped content (`crate::diff_shape::section_start_line_for_symbol`),
so the first-match rule now also decides which symbols get a working
auto-scroll target and which get none.

Dogfooding on PR #86 (a real, unmerged PR adding
`rinkaku-core/src/file_size.rs`, a **brand-new file**) surfaced a
concrete failure of that assumption: moving the left-pane cursor
between symbols in `file_size.rs` did not scroll the diff pane at all,
for every symbol except the first one in the file. A reproduction
built from the real diff and real file content confirmed the exact
mechanism —

- A brand-new file's diff is always exactly **one hunk**
  (`@@ -0,0 +1,N @@` spanning the whole file), regardless of how many
  symbols the file defines.
- `build_file_content`'s owner lookup
  (`symbols.iter().position(|s| hunk_intersects(hunk, s.range.start, s.range.end))`)
  finds the *first* symbol whose range intersects a hunk and attributes
  the whole hunk to it, full stop.
- Every symbol in the file has a range inside `[1, N]`, so every one of
  them "intersects" this one hunk — but only the first symbol in source
  order ever wins the `position()` lookup. Every other symbol's section
  gets zero hunks and is dropped by
  `sections.retain(|section| !section.hunks.is_empty())`.
- `section_start_line_for_symbol` then returns `None` for every
  dropped symbol, so `crate::run_app`'s `auto_scroll_for_diff_focus`
  (ADR 0027 decision 4) silently no-ops — the cursor moves, nothing
  scrolls, and there is no error or log to explain why.

This is not limited to free functions (an initial hypothesis while
investigating) — the reproduction shows a `struct` and an `enum` in
the same file are equally affected; the only fixed property is "this
symbol did not happen to be first". It is also not limited to brand-new
files in principle: any time git collapses a multi-symbol region into
one hunk (a large formatting pass, a block indent change, a
paste-and-adjust edit spanning several definitions) the same
first-match rule silently drops every symbol after the first from the
diff pane's per-symbol view and from auto-scroll — new files are simply
the case where this is *guaranteed* to happen on every symbol but the
first, rather than a rare adjacency accident.

## Decision

**Attribute a hunk to every symbol whose range it intersects, not just
the first.** `build_file_content`'s owner lookup changes from
`Iterator::position` (single index) to `Iterator::filter` (all matching
indices); the hunk (cloned once per matching symbol, as
`AttributedHunk` already is per section) is pushed onto every matching
symbol's section. A hunk that intersects no symbol still falls back to
the `MODULE_LEVEL_TITLE` bucket, unchanged.

This amends ADR 0020 decision 4's "attributed to the first symbol
(source order)" rule and confirms ADR 0027's decision 6 (`]c`/`[c`
walks every hunk in the file) needs a companion rule: **hunk-jump order
is *not* deduplicated by `source_index`** — it stays one stop per
section, exactly as `hunk_start_lines` already computes it today.
`hunk_start_lines` already walks sections in
`crate::ui::draw_diff_pane`'s render order and, unchanged, emits the
*same* hunk's start line once per section it now appears in (e.g. a
hunk shared by three adjacent symbols makes `]c` stop three times on
what the reviewer sees as one hunk of text, since the section headers
differ but the hunk body is identical content shown three times in the
render itself). This ADR accepts that the **rendered content itself
repeats the shared hunk once per owning section** (see Consequences) —
the diff pane is not deduplicating the *display*, since a section
without its own copy of the hunk would look incomplete when read on
its own (exactly the "orient before reading" principle ADR 0020 built
the section headers around) — so `hunk_start_lines` needs no code
change at all: its existing per-section walk already exposes a
distinct stop per **rendered** occurrence, so `]c`/`[c` continues to
mean "jump to the next hunk header actually on screen", matching what
the reviewer sees rather
than a deduplicated count that would silently skip over a hunk's
second/third rendered appearance.

**Why the TUI departs from ADR 0020's summary-view reasoning here:**
ADR 0020's rejection of duplicate attribution was scoped to "the
file-level view meant to summarize" change size — true of a
`## Definitions` / hotspot-style Markdown or JSON summary, where a
reader sums line counts across entries and a duplicate would inflate
that sum. The TUI diff pane is not that view: it has no "total lines
changed" figure anywhere in its own UI (`crate::ui::status`, `crate::detail`
checked — neither surfaces a change-size total derived from
`DiffSection` counts), and its purpose per ADR 0020's own framing is
"orient (what changed at the signature level) before reading the
body" *per selected symbol*. A reviewer who selects `compute_file_size_warnings`
and sees nothing scroll, with no indication that the function's own
hunk exists but was claimed by a sibling section, cannot orient at
all — the contract this ADR restores (every present symbol's row
"just works" for auto-scroll and per-symbol viewing) outweighs the
"hunk shown twice when two symbols share a region" cost, which is
visible and self-explanatory (the reviewer can see the same lines
appear under both section headers) rather than silent like the bug
this ADR fixes.

**No change to `rinkaku-core`.** This is purely a `rinkaku-tui`
diff-pane presentation concern — `crate::diff_shape` is TUI-only
(confirmed: `rinkaku-core`'s `render::markdown`/`render::report`/JSON
output do not depend on `rinkaku-tui` or read `DiffSection` at all;
they compute their own change-size figures, if any, directly from
`Report`/`ExtractedSymbol`). Markdown and JSON output are unaffected by
this ADR.

## Alternatives

- **Keep first-match, add a distinct "no section" indicator in the
  Detail pane or status line instead.** Rejected: this treats the
  symptom (auto-scroll doing nothing, silently) rather than the cause
  (the symbol's own hunk exists but was never attributed to it). A
  reviewer would still be unable to see `compute_file_size_warnings`'s
  own diff lines highlighted under its own header; an indicator only
  explains the absence, it does not restore the missing content.
- **Split a multi-symbol hunk into per-symbol sub-hunks at attribution
  time**, so each symbol's section shows only its own lines rather than
  the whole shared hunk. Rejected as substantially more complex: hunk
  bodies interleave context/added/removed lines
  (`crate::diff_view::DiffLine`) with no per-line symbol association
  already computed, so a correct split would need a second, finer
  intersection pass per line — a bigger change than this ADR's problem
  (auto-scroll silently doing nothing) requires solving. Can be
  revisited if reviewers find the duplicated-hunk-body a real annoyance
  in practice (see Consequences).
- **Attribute the hunk only to the symbol whose range it is most
  contained within (best-match instead of first-match or all-match).**
  Rejected for new-file hunks specifically: the one `@@ -0,0 +1,N @@`
  hunk is not "mostly contained" in any single symbol — it spans the
  entire file by construction, so a containment heuristic degenerates
  to an arbitrary tie-break no more principled than first-match, while
  adding more logic to reason about.
- **Special-case brand-new files only (detect `old_path` absent /
  a single hunk covering the full symbol table) and duplicate
  attribution only in that case.** Rejected: the same silent failure
  can occur on an existing file whenever git happens to merge several
  symbols' changes into one hunk (Context's "large formatting pass"
  example) — narrowing the fix to new files would leave that case
  unfixed for no simplification benefit, since the general "intersects
  more than one symbol" check already subsumes the new-file case
  without a special branch.

## Consequences

- Every symbol row in the TUI tree now gets a working auto-scroll
  target whenever its range intersects at least one hunk — including
  every symbol in a brand-new file, not just the first.
- A hunk shared by two or more adjacent/overlapping symbols is now
  rendered once per owning section instead of once total: a reviewer
  scrolling through a file selection sees the same lines appear under
  more than one section header. This is a deliberate, visible trade
  documented above, not a bug; it only occurs when symbol ranges
  genuinely share a hunk, which ADR 0020's own Context already flagged
  as an edge case ("possible when two symbols are adjacent with no
  gap") rather than the common case.
- `hunk_start_lines`'s hunk-jump table (`]c`/`[c`) now has one entry per
  **rendered** hunk occurrence rather than one entry per underlying
  diff hunk — a shared hunk yields multiple consecutive jump stops
  (one per section it was duplicated into), matching what is actually
  on screen at each stop.
- `rinkaku-core`'s Markdown/JSON output and their own change-size
  figures are untouched by this ADR — confirmed no shared code path
  with `crate::diff_shape`.
- No backward-compatibility concern: the TUI has never shipped a
  release (ADR 0015/0016, restated by ADR 0020 and ADR 0027), so this
  amendment applies in place rather than needing a migration.
