# 0044. Split (side-by-side) view for the diff pane

- Status: accepted
- Date: 2026-07-14

## Context

The diff pane (ADR 0020) has only ever rendered unified hunks: `-`/`+`
lines interleaved in file order, one column. Dogfooding on larger
signature-shape changes (a parameter reordered, a struct field renamed
across several adjacent lines) found unified rendering hard to read for
exactly that shape of edit — the old and new versions of a changed
block are not visually aligned, so spotting *which* token changed
requires the reader to hold both versions in their head while scanning
down a single interleaved column. A side-by-side (old-left, new-right)
view is the standard mitigation every major diff UI (GitHub, GitLab,
most editors) offers as an alternative, not a replacement, to unified —
some edits (a single added/removed line, a whole-file rewrite) read
better unified, others read better split, and the reviewer is in the
best position to judge per hunk.

Two invariants from prior ADRs constrain the design:

- **ADR 0027/ADR 0030's scroll-sync is line-index-based.**
  `crate::diff_shape::hunk_start_lines`/`section_start_line_for_symbol`/
  `symbol_id_for_scroll_line` all operate on one shared "logical line"
  coordinate space that `crate::ui::diff_pane::diff_pane_lines` renders
  1:1 and `crate::ui::scroll::render_scrollable_pane` scrolls through.
  Any split-view design that introduces a *second*, differently-sized
  coordinate space (e.g. one row per aligned pair, but a pair can absorb
  a variable number of unified lines when a run is re-flowed) would need
  either a second copy of all three lookup functions or a translation
  layer between the two spaces — real complexity this ADR wants to
  avoid if a simpler shape is available.
- **`crate::diff_shape` must stay free of `ratatui` types** (its own
  module doc comment, ADR 0020 decision 5) — pairing logic belongs
  there as plain data, rendering stays in `crate::ui::diff_pane`.

## Decision

**1. Toggle key: `v` / `V`**, both mapped to a single
`InputKey::ToggleSplitView`, global regardless of focus (mirroring
`d`/`D`'s ToggleDiff and `r`/`R`'s ToggleBlastRadius — a per-`App` mode,
not a per-row one). Chosen because neither `v` nor `V` was already bound
(confirmed by reading every `KeyCode::Char` arm in
`rinkaku-tui/src/lib.rs`'s `translate_key`) and it is the closest
single-letter mnemonic to the feature's own name ("split view") without
colliding with `s`/`S` (already `InputKey::Source`).

**2. `App` gains a `diff_view_mode: DiffViewMode` field** (`Unified` |
`Split`, `Default` = `Unified` — unified is what every prior ADR's
screenshots/dogfooding already assumes, so it stays the opening state).
`InputKey::ToggleSplitView` flips it; like `RightPane`, it is a mode
independent of the current row selection, so moving the cursor after
toggling keeps showing split (or unified) for the newly selected
row/file.

**3. Pairing is a new pure function in `crate::diff_shape`,
`pair_hunk_lines`, producing one `SplitRow` per *unified logical line*
— not one row per aligned old/new pair.** This is the direct consequence
of the first constraint in Context: reusing `hunk_start_lines`/
`section_start_line_for_symbol`/`symbol_id_for_scroll_line` unchanged
requires the split view's row count and row order to be identical to
the unified view's. Concretely:

- A `Context` line produces one `SplitRow { left: Some(line), right:
  Some(line) }` — the same text on both sides.
- A maximal run of consecutive `Removed` lines immediately followed by a
  maximal run of consecutive `Added` lines (the standard unified-diff
  "replace" shape) is paired positionally: row `i` of the run gets
  `left: removed_run.get(i)`, `right: added_run.get(i)` — `None` on
  whichever side ran out first when the two runs have different
  lengths (a pure insertion or deletion within the run). This produces
  exactly `max(removed_run.len(), added_run.len())` rows for the run,
  which can be *fewer* rows than the unified view's `removed_run.len()
  + added_run.len()` — see Decision 4 for how the shared line-index
  space absorbs that.
- A `Removed` run with no following `Added` run (pure deletion) or an
  `Added` run with no preceding `Removed` run (pure insertion) pairs
  every line against `None` on the other side, one row per line — no
  length mismatch to resolve.

**4. The shared line-index space is preserved by making `SplitRow`
itself the thing both views render per logical line — including a
`None`/`None` filler row for a unified line `walk_sections`
already counted but a merged split pair consumed.** (Amended below:
"mode-aware row counts for the section anchor" — a changed-signature
section anchor now legitimately renders a different row count in
unified vs. split, so `walk_sections` and its three consumers take a
`DiffViewMode` parameter; every other line in a section, including
every hunk body line, is still governed by this decision unchanged.)
Concretely,
`pair_hunk_lines` returns exactly `hunk.lines.len()` `SplitRow`s, one
per original `DiffLine`, not `max(removed, added)`: when a paired-off
`Removed`/`Added` pair merges onto one *rendered* split row, the
*second* line's `SplitRow` in source order carries `left: None, right:
None` (a blank filler row) rather than being dropped, so the total row
count — and therefore every offset `walk_sections` already computes —
stays identical to the unified count. This is a deliberate trade
(Alternatives below): it costs a few blank filler rows inside a
replaced run in split mode, in exchange for zero changes to
`hunk_start_lines`, `section_start_line_for_symbol`,
`symbol_id_for_scroll_line`, or `walk_sections` itself, and zero risk of
desyncing ADR 0027's tree→diff auto-scroll or ADR 0030's diff→tree
sync when the view mode is toggled mid-session.

**5. Rendering.** (Amended below: "mode-aware row counts for the
section anchor" replaces this decision's contract-header/filler-row
layout — the section title and the contract header are no longer
separate scaffold elements, so there is no filler row to keep a
2-line budget intact. The rest of this decision, the plain
title/hunk-header scaffold and the highlighting lookup, is unchanged.)
`crate::ui::diff_pane::draw_diff_pane` branches on
`app.diff_view_mode()`: unified keeps calling `diff_pane_lines` exactly
as today; split calls a new `diff_pane_split_rows`, which produces the
same section/header/hunk-header scaffolding as `diff_pane_lines` and
renders it inside a horizontal 50/50 `Layout::horizontal` split of the
pane's body area (inside `render_scrollable_pane`'s existing body — see
Decision 6 on why `render_scrollable_pane` itself needs one small
extension, not a parallel implementation). A title/hunk-header scaffold
line renders identically on both sides (`left`/`right` share it,
needing no special case), but the contract header's 2-line old/new
signature pair is the one scaffold element split view treats
differently from unified: both signatures render on the *same* row
(`left` = old, `right` = new) rather than unified's two separate
`-`/`+` lines, with a blank filler row below to keep the section's
2-line contract-header budget intact for the shared line-counting
Decision 4 relies on — putting the two signatures on separate rows
(mirroring unified's own line order) would reintroduce the exact
"scan past an interleaved line to compare" problem this whole ADR
exists to fix, this time inside the one scaffold element a reviewer
most wants aligned. Old-side lines keep the `-`/red styling, new-side
lines keep `+`/green, a filler cell renders as a blank styled line —
no new color semantics, reusing `diff_line`/`marker_span`/
`plain_diff_line`'s existing per-`DiffLineKind` styling and ADR 0018's
highlighting lookup (by `source_index`, unchanged) on whichever side has
real content.

**6. `render_scrollable_pane` gains a `Body` enum parameter
(`Single` | `Split`) rather than a second function.** `Single` is
today's exact behavior (one `Paragraph`, unchanged). `Split` lays the
already-wrapped body out as two side-by-side `Paragraph`s sharing one
`(scroll as u16, 0)` offset — wrapping happens independently per side
(each side's own text can wrap to a different number of visual rows at
a narrow width), so `Split` wraps the *logical* rows first with each
side's own half-width, then re-pads the shorter side's wrapped output
so both columns still agree on a total visual-row count before
scrolling — mirroring how the existing `wrap_lines` call already has to
run before `clamp_scroll`/`scroll_indicator` for the unified case (this
module's own doc comment). This keeps the clamp/indicator math — and
the `header_lines` split above the scrollable body — shared code, not
duplicated.

**7. Narrow-terminal fallback: below `MIN_SPLIT_VIEW_WIDTH` (100
columns for the diff pane's own area, chosen so each side gets roughly
an 80-column-equivalent budget after the border and a 1-column gutter),
`draw_diff_pane` renders unified regardless of `diff_view_mode`, with a
one-line dim note appended to the pane header
(`"(split view needs a wider pane)"`)** — rather than rendering an
unreadably narrow split. The toggle key itself still flips
`diff_view_mode` (so widening the terminal or the pane immediately
shows split without needing to press `v` again), matching how other
panes in this crate degrade gracefully rather than refusing input
outright.

**8. Scroll-sync (ADR 0027/0030), hunk-jump (`]`/`[`), and highlighting
(ADR 0018) all continue to operate on the shared logical-line coordinate
space unchanged — no new code path for any of them.** This is the
direct payoff of decisions 3–4: `App::right_pane_scroll`,
`hunk_start_lines`, `section_start_line_for_symbol`,
`symbol_id_for_scroll_line`, and `highlight::lookup_hunk_highlight_by_index`
are all called exactly as they are today regardless of
`diff_view_mode`; only `diff_pane_lines` vs. `diff_pane_split_rows` (and
`render_scrollable_pane`'s new `Body` parameter) differ between the
two modes.

## Alternatives

- **One row per aligned pair (`max(removed, added)` rows per run,
  fewer total rows than unified) instead of decision 4's filler-row
  padding.** Rejected: this is the "second coordinate space" problem
  Context calls out — `hunk_start_lines`/`section_start_line_for_symbol`/
  `symbol_id_for_scroll_line` would need a second implementation (or a
  translation table) for split mode, and toggling `v` mid-scroll would
  need to convert the current `right_pane_scroll` between the two
  spaces to avoid the reviewer's position jumping. A few blank filler
  rows inside a replaced run is a small, visible, well-understood cost;
  a second scroll-coordinate space is an ongoing maintenance and
  correctness burden across three ADRs' worth of existing sync logic.
- **A real Myers-diff-style token/line alignment (LCS-based), instead
  of decision 3's positional pairing within same-kind runs.** Rejected
  as over-engineering for this pane: unified diff hunks already come
  from git's own line-level diff, so within one hunk there is no
  "which old line corresponds to which new line" ambiguity left to
  resolve beyond grouping consecutive removed/added runs — a second diff
  algorithm on top of git's own output would re-litigate a decision git
  already made, for a pane whose job is presenting git's hunks, not
  re-diffing them.
- **Compute two independent row counts (unified vs. split) and give
  `App` a per-mode scroll offset.** Rejected: doubles the state
  `App::right_pane_scroll`'s own doc comment already carefully
  documents (unclamped request, clamped at draw time, folded back after
  every frame — ADR 0020's per-frame-recompute lesson), and a reviewer
  toggling `v` mid-read would need a defined rule for what the *other*
  mode's offset should be when they toggle back, a design surface this
  ADR's single shared coordinate space avoids needing at all.
- **Render split as two entirely separate bordered panes (old pane,
  new pane) instead of one pane with an internal 50/50 split.**
  Rejected: doubles the border/title/header chrome for content that is
  one logical diff, and `render_scrollable_pane`'s single-scroll-offset
  contract (decision 6) would need to be duplicated across two
  `Frame::render_widget` calls with hand-synchronized scroll state —
  more surface for the same result the internal split achieves with one
  `Layout::horizontal` call.
- **No narrow-terminal fallback; let split render at any width, however
  cramped.** Rejected: a diff pane narrower than ~50 columns per side
  wraps every real code line multiple times, defeating the whole
  point of side-by-side alignment (rows no longer visually line up once
  either side wraps to a different visual-row count than the other) —
  decision 7's threshold keeps the feature only available when it can
  actually deliver the readability win it exists for.

## Consequences

- A reviewer can toggle between unified and split rendering per
  session with `v`/`V`, independent of which row is selected — the same
  ergonomics `d`/`D` (Detail/Diff) and `r`/`R` (BlastRadius) already
  established for other per-`App` display modes.
- `crate::diff_shape` gains one new pure function (`pair_hunk_lines`)
  and one new type (`SplitRow`), unit-tested the same way
  `hunk_start_lines`/`section_start_line_for_symbol` already are — no
  new `ratatui` dependency in that module.
- `crate::ui::diff_pane` gains a second line-building function
  (`diff_pane_split_rows`) alongside `diff_pane_lines`, and
  `crate::ui::scroll::render_scrollable_pane` gains one new parameter
  (`Body`) — every existing call site (Detail pane, Blast-radius pane,
  help overlay, jump popup) passes `Body::Single`, matching today's
  behavior exactly; only the Diff pane's split-mode call site uses
  `Body::Split`.
- Split mode's rendered row count for a replaced run can include blank
  filler rows the unified view never showed (decision 4) — a visible,
  deliberate trade for keeping every prior ADR's scroll-sync code
  unchanged, not a bug.
- Terminals narrower than `MIN_SPLIT_VIEW_WIDTH` never actually render
  split, regardless of the toggle state — this is a graceful
  degradation, not a silent failure (the pane header notes why).
- No backward-compatibility concern: the TUI has never shipped a
  release (ADR 0015/0016, restated by every TUI-scoped ADR since), so
  this is a pure addition, not a migration.

## Amendment: default flipped to `Split`

Decision 2 originally defaulted `DiffViewMode` to `Unified`, reasoning
that every prior ADR's screenshots assumed unified rendering. Further
dogfooding after the toggle shipped found the opposite: split is the
more useful *opening* state for the pane's typical case (a signature or
small block edit, exactly what the split view exists to make legible),
and reviewers were pressing `v` immediately on most sessions anyway.
`DiffViewMode::default()` now returns `Split`. Decision 7's narrow-
terminal fallback (`MIN_SPLIT_VIEW_WIDTH`) already renders unified
whenever the pane is too narrow for split, independent of
`diff_view_mode` — so this default change carries no new risk for
narrow terminals, only for wide ones where split was already available
a keypress away.

## Amendment: similarity-based alignment within a run

Decision 3's positional pairing (row `i` of a replace run gets the
run's `i`-th removed/added line) and the Alternatives section's
rejection of "a real Myers-diff-style token/line alignment" both
assumed a replace run's removed and added lines correspond 1:1 by
position — reasonable when git's own line-level diff already resolved
which old line corresponds to which new line, as Alternatives argued.
Dogfooding surfaced a run shape where that assumption breaks: lines
inserted *ahead* of an otherwise-unchanged line (e.g. a doc comment
added above an unchanged function signature) shift every position
after the insertion, so positional pairing puts the unchanged
signature's `Removed` row next to the *comment's* `Added` row instead
of the signature's own — the exact "scan past unrelated content to
find the real counterpart" problem this whole ADR exists to fix,
reintroduced inside the pane meant to fix it.

The Alternatives section's rejection was about *re-diffing git's
output* (finding correspondence git's line-level diff didn't already
resolve) — that reasoning does not cover this case, where the
correspondence *is* resolvable (the signature line is still
recognizably the same line) but positional pairing discards the
signal by only ever comparing same-offset lines.

**`crate::split_pairing::pair_hunk_lines`'s per-run pairing
(`crate::diff_shape::pair_hunk_lines` before this amendment split the
module) is now a similarity-based alignment**, not pure position:

- Each removed/added run pair is scored with a Needleman-Wunsch-style
  alignment DP (gap cost 0) over per-line similarity — the Jaccard
  index of each line's whitespace-split token set, `0.0..=1.0`.
- Only pairs scoring at or above `SIMILARITY_THRESHOLD` (`0.5`) are
  matched; an unmatched line becomes its own row against `None` on the
  other side, preserving order.
- **Two fallbacks reproduce the pre-amendment behavior exactly**,
  rather than degrading alignment quality below what positional
  pairing already offered: a run pair with zero matches above
  threshold (no similarity signal to exploit) and a run longer than
  `SIMILARITY_ALIGNMENT_MAX_RUN_LEN` (`200`, avoiding the DP's
  quadratic cost on a rare, very large replace) both call the extracted
  `positional_pairing` helper directly.
- The total-row invariant (decision 4) is preserved by construction:
  one filler row per *matched pair* (generalizing the old "one filler
  row per positionally-paired row" count), so
  `crate::diff_shape::walk_sections`/`hunk_start_lines`/
  `section_start_line_for_symbol`/`symbol_id_for_scroll_line` still
  need no changes — this amendment only changes *which* lines end up
  paired on a row, never how many rows a run produces.

This grew the pairing logic past what fits comfortably alongside
`crate::diff_shape`'s section-building and unified-view line counting
(CLAUDE.md's file-size discipline), so `SplitRow`/`pair_hunk_lines`
and their helpers moved to a new sibling module,
`crate::split_pairing`, re-exported from `crate::diff_shape` so every
existing `crate::diff_shape::{SplitRow, pair_hunk_lines}` call site is
unchanged.

Token-based similarity (not a character diff) was chosen so
reindentation or a single changed argument does not swamp the score,
and order-insensitive token-set overlap (not a positional token
comparison) so a reordered clause still scores high — both suit
comparing source lines, whose meaningful unit of change is the token.

## Amendment: mode-aware row counts for the section anchor

Decision 4's shared line-index space assumed every rendered element —
including the section title and, separately, a changed symbol's
2-line contract header below it — has the same row count in unified
and split. A follow-up dogfooding pass found the contract header's own
styling (plain foreground-only red/green text, no background) didn't
read as a diff at a glance, unlike every other `+`/`-` line in the
pane (ADR 0018's background-tint convention). Fixing that surfaced a
better layout: the section anchor a reviewer lands on when jumping to
a symbol should show the diff itself, not a plain title with a diff
summary bolted on below it. So a changed signature's contract header
now *replaces* the section title outright, and unified/split render
that replacement differently:

- **Unified** shows a 2-line `- {old}` / `+ {new}` pair standing in
  for the title, each carrying the same `ADDED_BG`/`REMOVED_BG` tint
  ordinary diff lines use.
- **Split** pairs `{old}` (left) and `{new}` (right) onto the title's
  own single row — split view already had no use for the extra filler
  row decision 5's old contract-header layout budgeted for, since old
  and new sit side by side rather than stacked.

An unchanged section's title is unaffected in both modes (still one
plain bold row). So a changed-signature section's anchor is 2 rows in
unified but 1 row in split — the first case in this ADR where the two
modes legitimately disagree on a row count for the same section,
something decision 4 did not anticipate and could not, by
construction, absorb: there is no unified-side line for split's single
paired row to correspond to.

**`crate::diff_shape::walk_sections` and its three public consumers
(`hunk_start_lines`, `section_start_line_for_symbol`,
`symbol_id_for_scroll_line`) now take a `DiffViewMode` parameter.**
Each computes the section-anchor row count as 2 (unified, changed
signature), 1 (unified, unchanged title), or 1 (split, either case),
before continuing with decision 3/4's unchanged per-hunk counting.
Callers pass `App::diff_view_mode()` — the *requested* mode, i.e. what
`v`/`V` last toggled — not the pane's possibly-narrower *effective*
mode after decision 7's `MIN_SPLIT_VIEW_WIDTH` fallback silently
renders unified anyway. Threading the effective mode through instead
would require plumbing the diff pane's `Rect` width into
`crate::run_app`'s key-dispatch layer, which today has no notion of
pane geometry at all — a materially larger change for a narrow-terminal
edge case.

**Accepted trade-off:** when the pane is narrower than
`MIN_SPLIT_VIEW_WIDTH` and the reviewer has toggled to split, a
hunk-jump (`]`/`[`) or symbol-scroll target computed from the
requested (`Split`) row count can be off by one row per
changed-signature section actually rendered as unified. This does not
desync the diff pane from the tree cursor and does not corrupt
rendering — `crate::ui::clamp_scroll` already absorbs any resulting
overscroll the same way it absorbs any other requested-vs-actual
mismatch, next frame. This is strictly narrower than decision 4's
original invariant, not a full supersession of it: every other
element a section renders (hunk headers, hunk body lines, blank
separators) still has one shared row count across both modes,
unchanged.
