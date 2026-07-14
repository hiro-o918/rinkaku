# 0046. Overlay the diff onto the source view instead of a full-file diff pane mode

- Status: accepted
- Date: 2026-07-14

## Context

The diff pane (ADR 0020) only ever shows the hunks touching the
selected row — for a symbol row, just that symbol's own lines; for a
file row, every hunk grouped by section. A reviewer who wants to see a
changed symbol in the context of its *whole file*, with the change
still marked, has no path to that today: the diff pane deliberately
never shows unchanged surrounding code, and the source view (`s`, ADR
0015) shows the whole file but has no notion of what changed in it —
every line renders identically regardless of whether the diff touched
it.

Two ways to close this gap were on the table:

1. Add a "full file" mode to the diff pane itself.
2. Overlay the diff's added/removed lines onto the existing source
   view.

The diff pane's line-index coordinate space is load-bearing: ADR
0027/0030's scroll sync (tree selection <-> diff pane scroll position)
and ADR 0044's split-view pairing both operate on "logical line
number within the rendered hunks," a coordinate space that only
includes lines the diff actually touched (plus a few lines of
surrounding context git includes in the hunk). Stretching that same
pane to show *every* line of the file — most of which are outside any
hunk — would require either a second, parallel coordinate space (the
exact problem ADR 0044's Context section already flagged and designed
around when it chose hunk-relative pairing over full alignment) or
teaching every consumer of today's hunk-relative space
(`hunk_start_lines`, `section_start_line_for_symbol`,
`symbol_id_for_scroll_line`) to also understand full-file line numbers.
Neither is a small change, and both risk desyncing scroll-sync logic
three prior ADRs went to specific lengths to keep simple.

The source view, by contrast, already renders every line of the file
(`crate::source::SourceView.lines`) and already composites one
line-level visual signal on top of syntax highlighting (ADR 0018's
amendment: the drilled-into symbol's own range gets a background
tint, composed with token foreground colors via
`ui::style::styled_content_spans`). Adding a second line-level
background tint — added/removed, this time keyed by the diff rather
than the symbol's own range — is the same composition technique
applied a second time, not a new mechanism.

## Decision

**1. The source view (`s`), not the diff pane, is the home for
full-file diff context.** When the drilled-into symbol's file has an
entry in the diff, the source view composites the diff onto its
existing full-file rendering: an **added** line gets `ADDED_BG`
(`crate::ui::diff_pane::ADDED_BG`) with a `+` gutter marker; a
**removed** line is inserted immediately before the new-side position
it used to occupy, gets `REMOVED_BG`, and a `-` gutter marker. Lines
the diff didn't touch keep the source view's existing plain gutter
(the line number) and no background tint, same as today.

**2. Always on, no toggle.** Every symbol's file either has diff
hunks or it doesn't; when it does, the overlay is exactly the
information a reviewer opening the source view from a changed symbol
wants to see, and there is no rendering this ADR is aware of that the
overlay would make *worse* for a hunk-having file (unlike ADR 0044's
split view, which trades width for alignment and genuinely isn't
always the better choice). A toggle would add a key binding and a
mode dimension for a feature with no real "I'd rather not see this"
case. If a real one surfaces later, a toggle can be added as an
amendment.

**3. Line-number mapping is a new pure function, `source_diff::hunk_overlay_lines`,
in a new module `rinkaku_tui::source_diff`.** Given one `Hunk` (already
parsed by `crate::diff_view`), it walks `hunk.lines` and computes each
line's position in the overlay: an `Added`/`Context` line's new-side
line number advances a running counter seeded from `hunk.new_range`'s
start (mirroring how `crate::diff_view::Hunk::new_range` itself is
derived — module doc comment there); a `Removed` line carries the
*next* new-side line number, meaning "insert immediately before this
line" — the same "deletion is a position, not a line" convention
`crate::diff_view::hunk_intersects` already established for a
pure-deletion hunk's `new_range`, reused here rather than inventing a
second one. `Context` lines are dropped from the overlay's output —
the source view's own lines already render them; the overlay only
needs to add information for `Added`/`Removed`.

**4. Composition into display rows is a second pure function,
`source_diff::overlay_source_lines`.** Given `SourceView.lines` and
the flattened overlay entries for every intersecting hunk in the
file, it produces one `Vec<OverlayRow>` — one entry per source line
(kind `Unchanged`, carrying that line's own 1-based number) plus one
extra entry immediately before an `Added` line's number for each
`Removed` overlay entry at that position (kind `Removed`, carrying no
source line number — nothing in `SourceView.lines` to point at,
consistent with `crate::diff_view::Hunk::new_range`'s own convention
that a deletion has a position but not a line of its own). An `Added`
line's entry is the existing `Unchanged` row at that line number,
re-tagged `Added`; the function does not duplicate it.

**5. Working-tree drift degrades to no overlay for that file, not a
best-effort mismatch.** `crate::source::load_symbol_source`'s own doc
comment already documents that the source view reads the live working
tree while `report`/`diff_text` may have been built from a historical
commit or stdin — the file's current content and the diff's hunks can
disagree about what a given new-side line number actually contains.
`overlay_source_lines` detects this the only way available without a
second file read: each hunk's `Context` lines carry the exact text
`crate::diff_view` parsed from the diff itself, so before compositing,
every context line's text is compared against `SourceView.lines` at
its computed line number. On the first mismatch, the whole file's
overlay is dropped (not just the one disagreeing hunk) and the source
view renders exactly as it does today, unmodified, with a one-line
note appended to the pane header: `"(diff overlay unavailable — file
on disk doesn't match the diff)"`. Silently overlaying a
partially-wrong mapping would show colored lines in the wrong place,
which is worse than showing no overlay at all; a per-hunk partial
overlay would leave the reviewer unsure which hunks in the same file
to trust.

**6. Rendering reuses the existing token-foreground / background-tint
composition.** `ui::source_screen::source_lines` already composes
`styled_content_spans`'s token foreground with the symbol's own
`SOURCE_HIGHLIGHT_BG` background tint; the diff overlay's
`ADDED_BG`/`REMOVED_BG` slot into the same `bg: Option<Color>`
parameter. When both signals would apply to the same line (a changed
line that also falls inside the drilled-into symbol's own range), the
diff's added/removed tint wins — it is the more specific, higher-value
signal for a reviewer who followed a changed symbol into its file
specifically to see what changed, and the symbol-range tint's whole
job (orienting the reader to "you are here") is already accomplished
by the gutter's line-number highlighting plus the initial scroll
position ADR 0026 already centers on the symbol.

## Alternatives

- **Full-file mode inside the diff pane.** Rejected per Context: reuses
  a coordinate space three prior ADRs (0027, 0030, 0044) built specific
  scroll-sync and pairing logic around, none of which anticipated
  "every line of the file, not just hunk lines." Doubling that space
  risks the exact class of bug ADR 0044's Alternatives section already
  called out and designed around once.
- **A toggle key for the overlay.** Rejected per decision 2: no
  identified case where a reviewer benefits from suppressing it once
  it's available, unlike split view's genuine per-hunk trade-off.
- **Best-effort overlay on working-tree drift, applied hunk-by-hunk.**
  Rejected per decision 5: a half-correct overlay (some hunks placed
  right, others silently wrong) is harder to trust than a clearly
  absent one with an explicit note.
- **Re-locate hunks against the live file via a text diff (e.g. a
  Myers diff between the hunk's recorded old-side text and the current
  file) instead of dropping the overlay on any mismatch.** Rejected as
  a second diffing algorithm layered on top of git's own hunks, the
  same "re-litigating a decision git already made" objection ADR
  0044's Alternatives raised against LCS-based split-view alignment —
  and the source view's own doc comment already accepts working-tree
  drift as a known, documented limitation for the symbol-range
  highlight; the overlay inherits the same limitation rather than
  solving a problem the base feature doesn't solve either.

## Consequences

- A reviewer pressing `s` on a changed symbol sees the change directly
  in the file, colored the same way the diff pane colors it, without
  losing the full-file context the diff pane deliberately never shows.
- `rinkaku-tui` gains one new module (`source_diff`), keeping
  `crate::diff_view`, `crate::source`, and `crate::ui::source_screen`
  each under the file-size discipline's watch threshold rather than
  growing an already-large file further.
- The overlay depends on `crate::diff_view::parse_diff_hunks`'s output,
  already computed once per session by `crate::run_app` — no new IO,
  no new tree-sitter parse, only a pure composition pass alongside the
  existing `load_highlighted_symbol_source` call the `s` key already
  triggers.
- A file edited on disk since the diff was produced silently loses its
  overlay (decision 5) rather than showing a misleading one — visible
  in the pane header, not a silent gap.
