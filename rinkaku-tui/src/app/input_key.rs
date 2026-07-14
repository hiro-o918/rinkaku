/// A user key press, already stripped of `crossterm`-specific detail
/// (repeat/release events, modifier bitflags irrelevant to this app) down
/// to exactly the variants the app reacts to. Built by `crate::run`'s
/// event loop from a real `crossterm::event::KeyEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKey {
    /// `j`/`k`/arrow keys: moves the tree cursor while [`Focus::Tree`], or
    /// scrolls the right pane by one line while [`Focus::Right`] (ADR 0020)
    /// — `App::handle_key` branches on `self.focus`, not on a distinct pair
    /// of variants, since the physical key is the same either way and only
    /// its target changes.
    Up,
    Down,
    /// Space while [`Focus::Tree`], or Enter on a directory row: expand/
    /// collapse a directory row (`App::handle_key`'s doc comment) — never
    /// changes focus. A no-op while [`Focus::Right`] (matching
    /// [`Self::Open`]'s own Tree-only reach, ADR 0020 finding: this used to
    /// fire regardless of focus, silently toggling the tree cursor's row
    /// behind whichever right-pane content was actually on screen). Kept as
    /// a distinct variant from [`Self::Open`] because Space must never move
    /// focus even on a file/symbol row, only Enter does.
    Select,
    /// Enter on a file/symbol row: switches the right pane to
    /// [`RightPane::Diff`] and moves focus to [`Focus::Right`] (ADR 0020's
    /// "drilling into a row is also a focus change") — a no-op on a
    /// directory row (`App::handle_key`'s doc comment; a directory row's
    /// Enter is [`Self::Select`]/`crate::run`'s `translate_key`, matching on
    /// `KeyCode::Enter`, always emits `Open` and lets `handle_key` decide
    /// what that means per row kind).
    ///
    /// Post-ADR-0020 dogfooding finding: this used to open
    /// [`Screen::Source`] for a symbol row specifically (reading the file
    /// straight from the working tree, `crate::source::load_symbol_source`)
    /// while a file row's Enter only changed focus — an asymmetry a
    /// reviewer could not predict from the keypress alone, and one that
    /// surfaced as an apparently random failure whenever the working tree
    /// did not have the symbol's file in the expected state (a deleted or
    /// not-yet-checked-out file, reported as "enter だと source を出そうとして
    /// エラーになったりならなかったりする"). Enter now always means "show me
    /// the diff for this row", which never touches the filesystem and so
    /// never fails; opening the source view (which can fail, since it reads
    /// a real file) stays behind the dedicated `s` key ([`Self::Source`])
    /// only.
    ///
    /// Map-assisted-review finding: while already [`Focus::Right`] and the
    /// right pane is already [`RightPane::Diff`] (i.e. Enter is pressed a
    /// second time on the same row, mid-read), this is a **complete no-op**
    /// — `App::handle_key`'s own doc comment on why this needs a dedicated
    /// `preserve_scroll` case, distinct from `Focus::Tree`'s "switch to Diff"
    /// case just below. Without this, Enter matched regardless of focus (the
    /// `(Screen::Entry, _, InputKey::Open)` arm), so pressing it again while
    /// deep in a long diff silently reset `right_pane_scroll` to 0 via the
    /// blanket end-of-function reset, throwing the reviewer's reading
    /// position away for no visible reason. While [`Focus::Right`] but the
    /// right pane is [`RightPane::Detail`]/[`RightPane::BlastRadius`], Enter
    /// still switches to Diff (a real pane change, so the existing
    /// scroll-reset-on-other-keys rule is the correct behavior there, not a
    /// bug).
    Open,
    /// `e`/`E`: expand every row.
    ExpandAll,
    /// `c`/`C`: collapse every row.
    CollapseAll,
    /// `o`: toggle topological/alphabetical ordering.
    ToggleOrder,
    /// `s`: open the source view on the row under the cursor (a symbol
    /// row only — see `App::handle_key`).
    Source,
    /// `d`/`D`: toggle the right-hand pane between [`RightPane::Detail`]
    /// and [`RightPane::Diff`] (TUI iteration 2) — a per-`App` mode rather
    /// than a per-row one, so switching to the diff pane on one row and
    /// then moving the cursor keeps showing the diff pane for the newly
    /// selected row instead of resetting on every cursor move. Global
    /// regardless of [`Focus`] (ADR 0020).
    ToggleDiff,
    /// `R`: toggle the right-hand pane between [`RightPane::BlastRadius`]
    /// and whichever mode was active before ([`RightPane::Detail`] or
    /// [`RightPane::Diff`]) — ADR 0019's entry-path re-rooting, named "blast
    /// radius" in the UI per ADR 0023. Pressing `R` again while already in
    /// `BlastRadius` mode returns to the prior mode (stored in `App`'s
    /// `blast_radius_return_pane` field the moment `BlastRadius` was
    /// entered), mirroring `d`'s own toggle rather than a one-way "enter
    /// blast-radius mode" action, since the ADR describes `R` as a per-row
    /// toggle. Global regardless of [`Focus`] (ADR 0020).
    ToggleBlastRadius,
    /// `h` or Esc while [`Focus::Right`]: returns focus to [`Focus::Tree`]
    /// (ADR 0020's neovim-style "move left/back"). A no-op while already
    /// [`Focus::Tree`] on the entry screen (nothing to return from) — Esc's
    /// other meaning, returning from the source screen, is the separate
    /// [`Self::Back`] variant; `crate::run`'s `translate_key` disambiguates
    /// by screen the same way it already does for `q`.
    FocusLeft,
    /// `]c` while [`Focus::Right`] and the right pane is [`RightPane::Diff`]:
    /// scrolls to the start of the next hunk in the shaped diff content
    /// (ADR 0020). A no-op outside that pane/focus combination.
    NextHunk,
    /// `[c`: the reverse of [`Self::NextHunk`].
    PrevHunk,
    /// Esc or `q` while in the source view: return to the entry view.
    /// A no-op on the entry view itself (`q`'s quit behavior on the entry
    /// view is `InputKey::Quit`, a separate variant, since Esc has no
    /// "back" target to return to there).
    Back,
    /// `Ctrl-d` (ADR 0026): scroll the reading pane down by half a viewport
    /// (`Screen::Source`, or [`Screen::Entry`] + [`Focus::Right`]). The
    /// actual step size depends on the pane's rendered height, which `App`
    /// does not know, so this variant is handled by [`App::handle_scroll_key`]
    /// (which takes the viewport height as an explicit argument) rather than
    /// [`App::handle_key`]. A no-op on [`Screen::Entry`] + [`Focus::Tree`]
    /// (nothing scrollable there — the tree already handles motion via
    /// [`Self::Up`]/[`Self::Down`]).
    ScrollHalfPageDown,
    /// `Ctrl-u`: the reverse of [`Self::ScrollHalfPageDown`].
    ScrollHalfPageUp,
    /// `gg` (ADR 0026, resolved by `crate::lib::translate_key`'s `pending_prefix`
    /// arm the same way `gd`/`gr` are, ADR 0022): scroll the reading pane
    /// to the top (line 0). Handled by [`App::handle_scroll_key`] alongside
    /// the half-page variants for uniformity, even though "top" itself does
    /// not need the viewport height.
    ScrollToTop,
    /// `G` (`Shift-g`, ADR 0026): scroll the reading pane to the bottom.
    /// Stored as `usize::MAX` and clamped down to
    /// `total_lines - viewport_height` at draw time, matching
    /// [`Screen::Source::scroll_top`]'s own sentinel convention.
    ScrollToBottom,
    /// `q` or Ctrl-C on the entry view: exit the application.
    Quit,
    /// `?`: opens the help overlay (ADR 0020). While the overlay is open,
    /// `?` instead closes it — `crate::run`'s `translate_key` maps the same
    /// physical key to this one variant either way, and `App::handle_key`
    /// treats it as a toggle.
    ToggleHelp,
    /// `g`, when no `g`-prefixed sequence is already pending (ADR 0022):
    /// records that `g` was just pressed so the very next key can resolve
    /// `gd`/`gr` (`crate::lib::translate_key` consults
    /// `App::pending_prefix` to do that resolution). Every key clears any
    /// previously pending prefix (`App::handle_key`'s own doc comment) —
    /// this variant is what *sets* it in the first place.
    ///
    /// Also included in `App::handle_key`'s `preserve_scroll` exception list
    /// (independent-review finding): being the leading key of the `gd`/`gr`
    /// sequence, `g` alone is dispatched through `handle_key` one keypress
    /// before `GotoDefinition`/`GotoReferences` itself — without this
    /// exception, `g`'s own blanket scroll reset would have zeroed
    /// `right_pane_scroll` before the following `d`/`r` even ran, defeating
    /// [`Self::GotoDefinition`]'s own fix for the same jumplist-scroll bug.
    PendingGoto,
    /// `gd` (a two-key sequence, ADR 0022): jump toward a callee of the
    /// symbol under the cursor. Candidate resolution needs `report`
    /// (`report.graph.edges`, via `crate::detail::symbol_mentions`), which
    /// `App::handle_key` does not have access to — mirroring
    /// `InputKey::NextHunk`/`PrevHunk`'s own precedent (that jump target
    /// needs the shaped diff content `App` also lacks), `crate::run_app`
    /// resolves candidates and applies the outcome itself, via
    /// `App::jump_to_symbol`/`App::open_jump_popup`/`App::set_status`. It
    /// still calls `App::handle_key(input_key)` first, same as every other
    /// key (`App::handle_key`'s own match arm for this variant is a no-op
    /// stub, but the unconditional `pending_prefix` clear at the top of that
    /// function is not — skipping the call entirely, an earlier version of
    /// this feature did, left `pending_prefix` stuck after every `gd`/`gr`
    /// press). Kept as its own `InputKey` variant (rather than folding the
    /// two `g`-prefixed keys into `PendingGoto`'s own resolution) so
    /// `crate::lib::translate_key`'s key-to-intent mapping stays legible
    /// independent of where the intent is actually processed.
    ///
    /// Independent-review finding: that same required `handle_key(input_key)`
    /// call (for `pending_prefix`) used to also run this function's blanket
    /// end-of-function scroll reset *before* `App::jump_to_symbol` recorded
    /// the jumplist entry, so every jumplist entry's saved scroll offset was
    /// always 0 and `Ctrl-o`/`Ctrl-i` could never actually restore a reading
    /// position — see `App::handle_key`'s `preserve_scroll`, which now
    /// special-cases these two variants for that reason.
    GotoDefinition,
    /// `gr`: the caller-direction mirror of [`Self::GotoDefinition`].
    GotoReferences,
    /// Ctrl-o: moves backward through the jumplist (ADR 0022) — the
    /// mirror-image of vim's own `Ctrl-o`. A no-op (with a status message)
    /// when the back-stack is empty.
    JumpBack,
    /// Ctrl-i: moves forward through the jumplist (ADR 0022), the reverse of
    /// [`Self::JumpBack`]. A no-op (with a status message) when the
    /// forward-stack is empty.
    JumpForward,
    /// Enter, while the jump-target popup (ADR 0022) is open: jumps to the
    /// popup's currently highlighted candidate and closes it. Reuses
    /// [`Self::Open`]'s physical key (`crate::lib::translate_key` maps Enter
    /// to `Open` regardless of context, mirroring how `?` already maps to
    /// the same [`Self::ToggleHelp`] variant whether the help overlay is
    /// open or not) rather than adding a dedicated variant only the popup
    /// would ever produce.
    PopupConfirm,
    /// Esc, while the jump-target popup is open: closes it without jumping.
    /// Reuses [`Self::Back`]'s physical key for the same reason
    /// `PopupConfirm` reuses `Open`'s.
    PopupCancel,
    /// `v`/`V` (ADR 0044): toggles the Diff pane between unified and split
    /// (side-by-side) rendering. A per-`App` mode, not a per-row one —
    /// mirrors [`Self::ToggleDiff`]/[`Self::ToggleBlastRadius`]'s own
    /// "global regardless of `Focus`" precedent, since this is a display
    /// mode for whichever content the Diff pane already shows, not an
    /// action on the row under the cursor.
    ToggleSplitView,
    /// `n` on the entry screen (ADR 0048): opens the review-note compose
    /// overlay over the row under the cursor. Needs a
    /// [`crate::review::SelectionSnapshot`] derived from `report`/the
    /// parsed diff hunks, which `App::handle_key` has no access to
    /// (mirroring [`Self::Source`]'s own "IO/derivation stays outside
    /// `App`" precedent) — `crate::lib::run_app` special-cases this
    /// variant before dispatch rather than routing it through
    /// `App::handle_key` at all, so this variant never reaches that
    /// method's match.
    NoteCompose,
    /// `N`: opens the review-notes list overlay (ADR 0048).
    NotesList,
    /// A printable character typed while the compose overlay is open.
    ComposeChar(char),
    /// Backspace while the compose overlay is open.
    ComposeBackspace,
    /// `d` while the notes list overlay is open: deletes the note under
    /// the list cursor.
    NoteDelete,
    /// `w`/`W` (ADR 0050): opens the current PR's page in the reviewer's
    /// default web browser. Global, like `d`/`r`/`s` — translated
    /// regardless of screen/focus. Needs the session's `PrContext`, which
    /// `App` does not hold (mirroring [`Self::NoteCompose`]'s own "IO/
    /// derivation stays outside `App`" precedent) — `crate::lib::run_app`
    /// special-cases this variant before dispatch rather than routing it
    /// through `App::handle_key`.
    OpenPrInBrowser,
}
