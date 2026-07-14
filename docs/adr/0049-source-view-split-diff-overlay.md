# 0049. Split (side-by-side) rendering for the source view's diff overlay

- Status: accepted
- Date: 2026-07-15
- Related: [ADR 0046](0046-source-view-diff-overlay.md) (introduces the
  unified diff overlay this ADR extends), [ADR 0044](0044-tui-split-view-diff-pane.md)
  (introduces `v`/`V` and the diff pane's own split view, whose row
  pairing this ADR reuses), [ADR 0047](0047-source-reader-port-for-pr-head-snapshot.md)
  (the `SourceReader` port this ADR's old-side reconstruction sits
  downstream of)

## Context

ADR 0046 composites a file's diff onto its full-file source view (`s`)
as an always-on unified overlay: added lines get a green background and
`+` gutter, removed lines are inserted as their own row with a red
background and `-` gutter. This reads well for an isolated one-line
change, but for the same "replace" shape ADR 0044 already identified in
the diff pane — a run of removed lines immediately followed by a run of
added lines, e.g. a renamed parameter or a reordered field — the unified
overlay interleaves old and new content in file order, so spotting which
token actually changed requires holding both versions in the reader's
head while scanning down one column. ADR 0044 solved exactly this for
the diff pane with a side-by-side split view; the source view's overlay
has the same problem and no equivalent.

Two things make this harder than reusing ADR 0044's split view as-is:

**1. The source view has no "old-side full text" to align against.**
The diff pane only ever renders hunk lines, so its split view has both
sides in hand directly from `Hunk::lines`. The source view renders the
*whole file*, and only the new side of that whole file is available
without extra IO — `crate::source::SourceView.lines` is read through
`SourceReader` (ADR 0047), which resolves to the working tree for
`--base`/stdin and the PR's head snapshot for `--pr`. There is no
committed "old-side whole file" content anywhere in `rinkaku-tui` for
any input mode: stdin mode has no git repository at all to `git show`
against, `--base` mode's base ref exists but reading it would be a
second `SourceReader`-shaped IO call this feature does not otherwise
need, and `--pr` mode's `PrHeadSourceReader` only ever fetches the head
ref, never the base.

**2. Unlike the diff pane, the unchanged majority of the file must also
render on both sides.** ADR 0044's split view only ever shows hunk
lines, where "unchanged" means a `Context` line mirrored onto both
sides. The source view's split mode must mirror the *entire file*,
because that full-file context is the whole reason ADR 0046 chose the
source view over a diff-pane mode in the first place (ADR 0046's
Context section). Old-side line numbers must therefore be tracked
across the whole file, not just inside hunks.

## Decision

**1. The old side is reconstructed by reverse-applying `FileHunks` onto
the new-side full text — no new IO.** A new pure function,
`source_split::reconstruct_old_lines(new_lines: &[String], file_hunks:
&FileHunks) -> Option<Vec<String>>`, walks the file once: for each
unchanged (non-hunk) stretch it copies `new_lines` through unchanged,
and for each hunk's body it walks `Hunk::lines` and emits `Removed`/
`Context` line content (in hunk order) as the old-side text, skipping
`Added` lines entirely — the inverse of how a unified diff is normally
*applied* forward. This works in every input mode (stdin, `--base`,
`--pr`) with the exact data already in hand (`SourceView.lines` plus
`crate::diff_view::parse_diff_hunks`'s already-parsed `FileHunks`),
because it needs no base-ref content at all — unlike a `git show
<base>:<path>` approach, which has no `<base>` to resolve in stdin mode
and would be a second `SourceReader`-shaped read this feature does not
otherwise need. Reverse-application also means old-side reconstruction
fails exactly when the existing overlay already fails: `Context` lines
that don't match `new_lines` at their expected position (the same drift
`crate::source_diff::overlay_source_lines` already detects) make
reconstruction ambiguous, so this returns `None` rather than guessing —
the same "no overlay is better than a wrong one" principle ADR 0046
decision 5 already established, not a new failure mode.

**2. Row pairing within a changed run reuses `split_pairing::pair_hunk_lines`
unchanged.** The source view's changed runs are the exact same
`Removed`-then-`Added` runs `Hunk::lines` already contains — this
ADR introduces no new pairing algorithm, just a new caller of the one
ADR 0044's amendment already built. This also means a signature edit
reads identically whether spotted in the diff pane's split view or the
source view's split view, rather than two independently-tuned
alignments.

**3. A new pure module, `rinkaku_tui::source_split`, builds one
`Vec<SourceSplitRow>` per file: a `SourceSplitRow` carries an optional
`(old_line_number, content)` on the left and an optional
`(new_line_number, content)` on the right, plus enough of `DiffLineKind`
to color it.** Unchanged stretches produce one row per line, old/new
line numbers advanced independently (they diverge across the file
exactly where prior hunks added or removed lines — an ordinary
line-number walk, not diff-specific math). Each hunk's changed run
produces rows via `pair_hunk_lines`, translated from `SplitRow`'s
hunk-relative shape into `SourceSplitRow`'s whole-file line numbers.
`None` on a side is a filler cell, rendered the same way the diff
pane's own split view renders one (`ui::diff_pane`'s existing
filler-row precedent, ADR 0044 decision 5) — no new "empty cell"
convention.

**4. `v`/`V` (`InputKey::ToggleSplitView`) now also applies to
`Screen::Source`, sharing the same global `App::diff_view_mode` the
diff pane already reads.** `rinkaku-tui/src/app/handle_key.rs`'s
`(Screen::Source { .. }, _, _) => {}` catch-all arm currently swallows
every key not explicitly matched above it, including `ToggleSplitView`
— a gap against `rinkaku-tui/src/help.rs`'s own `GLOBAL_BINDINGS` entry,
which already describes `v`/`V` as toggling "the Diff pane" specifically
(inaccurate once this ADR makes it a genuinely cross-screen toggle,
corrected as part of this change) rather than a screen-scoped one like
`d`/`D`/`r`/`R` (which are `Screen::Entry`-only by design, since Detail/
BlastRadius are `RightPane` concepts with no `Screen::Source`
equivalent). A dedicated `(Screen::Source { .. }, _, InputKey::ToggleSplitView)`
arm, placed before the catch-all, flips the same `diff_view_mode` field
`Screen::Entry`'s existing arm flips — one mode, one field, read by both
screens, matching `DiffViewMode`'s own doc comment ("independent of the
current row selection") which already covers screen changes as much as
row changes. `Screen::Source` therefore opens in split mode by default
too, same as the diff pane (`DiffViewMode::default()`, ADR 0044's
amendment) — no separate default for this screen.

**5. Rendering reuses `ui::scroll::{Body::Split, render_scrollable_pane}`
unchanged**, the same side-by-side layout, wrapping, and shared-scroll-
offset machinery the diff pane's split view already uses (ADR 0044
decision 6) — no second split-rendering implementation. Every *new*-side
cell — an unchanged row's mirrored content or a changed run's `Added`
line alike — reuses the already-computed `HighlightedSourceView` token
spans for that line number, the same lookup the unified overlay's `Added`
row already performs; the gate is "is this the new side," not "is this
row unchanged." The mirrored *old*-side content of an unchanged row
reuses the exact same span data, since old and new text are equal by
definition on an unchanged row. A changed run's `Removed` cells (old-side
only) render unhighlighted (plain text plus `REMOVED_BG`), matching the
unified overlay's existing `removed_line` treatment (ADR 0046 decision
6) — there is no parsed token data for old-side-only text in any mode,
split or unified alike.

**6. Narrow-pane fallback reuses `MIN_SPLIT_VIEW_WIDTH`
(`ui::diff_pane`).** Below that width, or when `reconstruct_old_lines`
returns `None` (drift, or a hunk-shaped input this reverse-application
can't resolve unambiguously), the source screen falls back to today's
unified overlay — the same title-note precedent ADR 0046 decision 5
already established for overlay-unavailable, extended to also cover
"overlay available, but not as a split."

## Alternatives

- **Read the old side via `git show <base>:<path>`.** Rejected per
  Context point 1: no base ref exists in stdin mode at all, and even in
  `--base`/`--pr` mode this would be a second file read alongside the
  `SourceReader` call the source view already makes, for content the
  diff's own hunks already fully determine without touching disk or git
  again.
- **A new alignment algorithm specific to whole-file split view,
  instead of reusing `pair_hunk_lines`.** Rejected: the changed-run
  shape inside a hunk is identical whether the caller is the diff pane
  or the source view, so a second algorithm would only risk the two
  views disagreeing on the same run's alignment — the exact drift ADR
  0044's amendment was designed to avoid for its own two fallback paths.
- **A toggle key of its own for this screen, independent of the diff
  pane's `v`/`V`.** Rejected: `DiffViewMode` is already documented as a
  per-`App`, cursor-independent mode; introducing a second toggle for
  the same "unified vs. split" concept, scoped to one screen, would
  give a reviewer two keybindings to remember for what reads as one
  preference, and would leave the pre-existing gap (the catch-all arm
  swallowing `ToggleSplitView` on `Screen::Source`) unaddressed rather
  than fixed.

## Consequences

- A reviewer viewing a changed symbol's full file (`s`) can now toggle
  the same `v`/`V` they already use in the diff pane to see a replaced
  block's old and new lines aligned side by side, instead of scanning
  an interleaved column.
- `rinkaku-tui` gains one new pure module, `source_split`, keeping
  `source_diff` (the existing unified overlay) and `ui::source_screen`
  under the file-size discipline's watch threshold rather than growing
  either further.
- No new IO, no new external dependency: old-side reconstruction is a
  pure function over data every input mode already has in memory
  (`SourceView.lines` plus already-parsed `FileHunks`).
- `v`/`V` is now accurately documented as a genuinely global toggle
  (`rinkaku-tui/src/help.rs`'s `GLOBAL_BINDINGS` entry updated), fixing
  a pre-existing gap between the help text and `handle_key`'s actual
  routing rather than introducing a new one.
- A file whose overlay is already unavailable (ADR 0046 decision 5's
  drift case) has no split view either, by construction — reconstructing
  an old side from a diff that doesn't match the file on disk is exactly
  as unreliable as overlaying it was.
