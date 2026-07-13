# 0030. Manual diff-pane scrolling syncs the tree cursor back to the visible symbol

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0027 made the tree cursor drive the diff pane: selecting a symbol
row auto-scrolls the right pane to that symbol's section (decision 2),
and decision 5 explicitly chose the opposite direction to be a
one-way street — "**Symbol-move preserves auto-scroll, not the manual
scroll offset**... A reviewer who scrolled by hand within one symbol's
section and then moves the cursor to a sibling symbol gets the
sibling's section start — not their prior manual offset translated
onto the new section." That decision was about what happens to the
*scroll offset* when the *cursor* moves; it did not say anything about
what should happen to the *cursor* when the reviewer scrolls the pane
by hand instead, because at the time nothing built that direction of
sync. In practice the two directions read as one-way sync: cursor
movement scrolls the pane, but scrolling the pane never updates the
cursor.

Dogfooding on ADR 0027/0029's own diff pane found this asymmetry
disorienting during an actual review pass: with `Focus::Right` and a
file selected, holding `j`/`k` (or the mouse wheel, ADR — mouse wheel
support, PR #84 — which is just `Up`/`Down` under a different input
source, per `translate_mouse_event`) scrolls down through several
symbols' sections in sequence, but the tree's cursor and Detail-pane-
style status stay frozen on whichever symbol was selected before the
scroll started. A reviewer who scrolls three sections down, then
presses `h`/`Esc` to return to `Focus::Tree` to open that symbol's own
Detail view or jump elsewhere, lands back on the *original* symbol
instead of the one they were just reading — exactly the "where am I"
problem a source-control web UI's split diff view does not have:
GitHub's/GitLab's file-tree sidebar highlights whichever file is
scrolled into view, so switching context (e.g. to comment, or open a
different file) starts from "where you actually are," not "where you
started." The diff pane's whole point (ADR 0015: TUI review surface)
is to let the reviewer treat scrolling and cursor movement as one
continuous "reading position," not two independently-tracked cursors
that can silently diverge.

## Decision

**1. Manual right-pane scrolling re-syncs the tree cursor to the
visible symbol's section, one-way (diff → tree), the mirror image of
ADR 0027's tree → diff sync.** After any key that changes
`right_pane_scroll` while `Focus::Right` and `RightPane::Diff` is
showing (`j`/`k`, the ADR 0026 half-page/top-bottom keys, `]c`/`[c`,
and mouse wheel — every one of them already funnels through the same
`right_pane_scroll` mutation, per `translate_mouse_event`'s own doc
comment "wheel input always acts on whichever pane already has
focus, exactly like a keyboard j/k press would"), `crate::run_app`
looks up which symbol section the new scroll offset's line falls
inside and, if that differs from the tree's current symbol, moves the
tree cursor there.

**2. The reverse lookup is a new pure function,
`crate::diff_shape::symbol_id_for_scroll_line`,** sitting next to
`section_start_line_for_symbol` (the forward lookup ADR 0027 already
added) and reusing the same `walk_sections` layout table — the two
functions are literal mirror images (`symbol_id -> start_line` vs.
`start_line -> symbol_id`) and belong in the same module for the same
reason `hunk_start_lines`/`section_start_line_for_symbol` already share
`walk_sections` rather than each re-deriving the layout.

**3. Lines with no owning symbol section do not move the cursor.**
Two cases produce this: the scroll offset sits inside the
`"(module level)"` bucket (a hunk touching no symbol — imports, a
module-level `use` change), or past the end of every section (an
overscroll about to be clamped by `crate::ui::clamp_scroll` next
frame, or briefly during a terminal resize). Both cases return `None`
from `symbol_id_for_scroll_line`, and `run_app` leaves the tree cursor
exactly where it was. Moving the cursor to *some* nearby symbol
anyway (nearest-section heuristics, "last symbol before this line")
was considered and rejected: the module-level bucket has no
`symbol_id` at all by construction (`DiffSection::symbol_id: None`,
ADR 0020 decision 4), so "the nearest symbol" would be a guess this
ADR has no principled way to make, and a guessed cursor jump reads as
more disorienting than simply not moving — the reviewer is looking at
content, just not any one symbol's content, and the tree cursor
staying put reflects that honestly.

**4. A tree row hidden under a collapsed ancestor is expanded, not
skipped.** Reuses `Nav::move_cursor_to_symbol`'s existing behavior
unchanged (ADR 0022's jump-navigation precedent: "a mid-session jump
target is very likely to be folded away by the time the reviewer
presses `gd`/`gr`... this method expands whatever stands in the way
instead of giving up"). The same reasoning applies here with more
force: `move_cursor_to_path`'s alternative "no-op if hidden" contract
exists for a *startup-time* pivot where a fresh, fully-expanded `App`
folding is a genuine caller error; a scrolling-driven sync happens
continuously mid-session, when the reviewer may well have collapsed
the very directory the pane is currently scrolled into (e.g. they
collapsed it after opening a different file's diff, then scrolled the
still-open pane back past that boundary). Silently refusing to sync
in that case would reintroduce exactly the "frozen cursor" problem
this ADR exists to fix, just gated on fold state instead of on
`Focus`.

**5. New method `App::sync_tree_cursor_to_symbol`, not
`App::jump_to_symbol`.** `jump_to_symbol` (ADR 0022) does three things
the scroll-sync must *not* do: it pushes the current position onto the
jumplist's back-stack, clears the forward-stack, and resets
`right_pane_scroll` to 0. All three are correct for an explicit
`gd`/`gr` jump (a deliberate navigation the reviewer should be able to
undo with `Ctrl-o`, landing on a fresh section top) and wrong for a
passive scroll-follow (recording every scroll-crossed symbol as a
jumplist entry would flood the jumplist with noise the reviewer never
asked to navigate through, and resetting the very `right_pane_scroll`
that triggered this whole sync would fight the scroll the reviewer
just performed). The new method only moves `self.nav` via
`Nav::move_cursor_to_symbol` (decision 4) and touches nothing else —
no jumplist, no scroll write.

**6. The feedback-loop guard: `last_diff_focus` must be updated by the
sync, not just read by it.** ADR 0027 decision 2's existing loop
(`crate::run_app`) already tracks `last_diff_focus` and only fires the
tree→diff auto-scroll when `app.selected_diff_focus(report)` differs
from it since the previous handled key — precisely to stop the
auto-scroll from re-firing on every idle-equivalent key and snapping
the pane back to the section top (that guard's own comment: "firing
auto-scroll unconditionally here would overwrite the reviewer's own
j/k... scrolling immediately after they pressed it"). Without this
ADR's sync writing its own new cursor position into `last_diff_focus`
*before* the loop iteration ends, the *next* handled key would see
`app.selected_diff_focus(report)` (now the synced symbol) differ from
the stale `last_diff_focus` (still the pre-scroll symbol), treat that
as a genuine cursor-driven selection change, and fire
`auto_scroll_for_diff_focus` — snapping `right_pane_scroll` right back
to the section start the reviewer had just scrolled past. `run_app`'s
new scroll→tree step therefore ends by setting
`last_diff_focus = app.selected_diff_focus(report)` (the post-sync
value) whenever the sync moved the cursor, closing the loop in the
same handled-key iteration rather than leaving it to resolve itself
one keypress later (verified by a regression test — see Consequences
— that a scroll immediately followed by another scroll key does not
reset `right_pane_scroll`).

**7. `h`/Esc back to `Focus::Tree` keeps ADR 0020's existing
scroll-reset rule unchanged.** `App::handle_key`'s blanket
"reset `right_pane_scroll` to 0 on every key except the `Focus::Right`
scroll exceptions" still runs for `InputKey::FocusLeft` — returning to
`Focus::Tree` resets the pane's scroll the same way it always has.
This ADR does not change that: the cursor now correctly reflects
where the reviewer scrolled to (decision 1), so pressing `h`/Esc opens
Detail/BlastRadius/whatever comes next for the *right* symbol; the
diff pane itself resetting to that symbol's own section top on the
next re-entry to `RightPane::Diff` (ADR 0027 decision 2's ordinary
auto-scroll) is the existing, desired behavior, not a regression this
ADR needs to touch.

## Alternatives

- **Two-way live binding (keep cursor and scroll offset as one
  continuously-reconciled value) instead of a one-shot sync per
  handled key.** Rejected as unnecessary complexity: `run_app`'s
  existing per-key dispatch loop (ADR 0020's caching discipline) only
  ever recomputes derived state once per handled key, never per draw
  frame — a "continuous" binding would still, in practice, resolve to
  exactly this ADR's per-key sync, just described with a fuzzier name.
- **Move the cursor to the nearest symbol when the scroll offset falls
  in the module-level bucket or past the end of content**, instead of
  leaving the cursor untouched (decision 3). Rejected: see decision
  3's own reasoning — no principled "nearest" definition exists for a
  bucket that structurally has no `symbol_id`, and a guessed jump is a
  worse UX than no jump.
- **Reuse `App::jump_to_symbol` directly, accepting the jumplist noise
  and scroll reset as a known cost.** Rejected outright: the scroll
  reset would make the sync fight its own trigger (scrolling to line
  40 would sync the cursor, which would reset scroll to 0, undoing the
  very scroll that triggered it) — not a tunable trade-off, a
  correctness bug. `App::sync_tree_cursor_to_symbol` (decision 5) was
  the only viable shape once that was clear.
- **Gate the sync on a debounce/threshold (e.g. only sync after the
  cursor has been inside a section for N frames) to avoid rapid
  cursor flicker during a fast scroll (holding `j` or a fast wheel
  spin).** Rejected: `run_app` only recomputes derived state once per
  *handled key* (decision 6's own point), and a human holding a key or
  spinning a wheel still generates one discrete `Up`/`Down` event per
  physical tick — there is no sub-key-press frame rate to debounce
  against in this architecture, so a debounce would add a state
  machine for a problem that structurally cannot occur here.
- **Sync eagerly inside `App::handle_key` itself** (folding the lookup
  into the same method that mutates `right_pane_scroll`) rather than
  as a separate step in `crate::run_app`. Rejected: `App::handle_key`
  has no access to `report`/`diff_pane_content` (the same reason
  `NextHunk`/`PrevHunk`'s hunk-jump target and ADR 0027's auto-scroll
  itself both already live in `crate::run_app` instead of `App` —
  `InputKey::NextHunk`'s own doc comment on why), so this ADR's sync
  joins that existing precedent rather than breaking it.

## Consequences

- The tree cursor and the diff pane's visible content can no longer
  silently diverge while `Focus::Right` is showing `RightPane::Diff`:
  scrolling past a symbol's section moves the cursor onto it, mirrored
  by ADR 0027's existing "selecting a symbol scrolls to it" direction.
  `h`/Esc back to `Focus::Tree` now reliably opens Detail/BlastRadius/
  Source for whichever symbol the reviewer was actually reading, not
  whichever symbol they started reading from.
- `crate::diff_shape` gains one new pure function
  (`symbol_id_for_scroll_line`) with its own unit tests, mirroring
  `section_start_line_for_symbol`'s existing test shape — no new
  `ratatui`/IO dependency, keeping the module's "pure view-model" scope
  unchanged.
- `crate::app::App` gains one new method
  (`sync_tree_cursor_to_symbol`) that is deliberately *not*
  `jump_to_symbol`: no jumplist entries are recorded for scroll-driven
  cursor moves, so `Ctrl-o`/`Ctrl-i` (jump back/forward, ADR 0022)
  continue to reflect only explicit `gd`/`gr` navigation, unpolluted
  by passive scrolling — a scroll session through ten symbols does not
  leave ten jumplist entries behind.
- `crate::run_app`'s loop body gains a new "did the scroll offset
  change and, if so, does it point at a different symbol" step. The
  diff-content-recompute-plus-both-sync-directions sequence (ADR 0027's
  auto-scroll and this ADR's scroll→tree sync) is extracted into its
  own pure `apply_diff_pane_selection_effects` function — mirroring why
  `dispatch_non_source_key` (ADR 0022) exists as a separate function at
  all: `run_app` itself takes a live `ratatui::DefaultTerminal` and
  cannot be driven directly in a test, so a bug in the *sequencing* of
  two dispatched keys (as opposed to a bug in one branch's own logic)
  would otherwise have no regression coverage. A dedicated test calls
  this function twice back-to-back, simulating two consecutive scroll
  keys, and asserts the second key's own scroll motion survives
  untouched — pinning decision 6's feedback-loop guard directly rather
  than only through code review.
- This PR also moves `crate::lib.rs`'s test module out to a sibling
  `tests.rs` (`#[cfg(test)] #[path = "tests.rs"] mod tests;`), the same
  co-location-but-not-counted pattern CLAUDE.md's file-size discipline
  documents and PR #82's `rinkaku-tui/src/app/{mod,input_key,tests}.rs`
  already established as this crate's canonical example — `lib.rs` had
  already crossed the 1500-line warn threshold (ADR 0028) primarily on
  test-code weight, and this ADR's own new tests would have pushed it
  further. Production code in `lib.rs` now sits at roughly 1100 lines
  (the 1000-1500 "watch" band, no action required); `tests.rs` itself
  is under the 2000-line split threshold. This is bookkeeping the ADR
  performs as part of implementing decision 6's new function, not a
  separate structural decision of its own.
- No backward-compatibility concern: the TUI has never shipped a
  release (ADR 0015/0016, restated by every TUI-scoped ADR since),
  so this amendment applies in place.
