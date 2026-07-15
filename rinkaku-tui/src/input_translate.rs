//! Raw `crossterm` input → this crate's terminal-agnostic [`InputKey`]
//! (ADR 0028 split out of `lib.rs`, which had grown past the file-size
//! threshold): [`translate_key`] for keyboard, [`translate_mouse_event`]
//! for mouse wheel, and [`normalize_fullwidth_key`], the full-width-ASCII
//! folding both `translate_key` (indirectly, via a Japanese/CJK IME) needs
//! before matching a plain-ASCII binding. Every function here is pure — no
//! IO, no `App` mutation — so `crate::run_app`'s event loop is the only
//! caller, feeding each translated [`InputKey`] into its own dispatch.

use crate::app::{self, App, InputKey, Screen};
use crate::review;
use crate::search::SearchMode;
use ratatui::crossterm::event::{self, KeyCode, KeyModifiers};

/// Translates a raw `crossterm` key press into this crate's
/// terminal-agnostic [`InputKey`], or `None` for a key the app does not
/// react to. Depends on `app.screen()` to disambiguate `q`/Esc (`Quit`/
/// `FocusLeft` on the entry view depending on focus, `Back` on the source
/// view) and on `app.focus()` (ADR 0020) to route Esc between `FocusLeft`
/// and its other meanings — every other mapping is context-free.
///
/// `app.help_open()` (ADR 0020) short-circuits every other rule: while the
/// help overlay is open, `?`/Esc/`q` all translate to `ToggleHelp` (closing
/// it) regardless of what they would otherwise mean, and this check runs
/// before every other arm so none of them — especially `q`, which would
/// otherwise mean `Quit` — can reach past the overlay. `App::handle_key`'s
/// own `help_open` guard is a second, independent layer of the same rule
/// (swallowing every non-`ToggleHelp` key while open) — belt and braces,
/// since "the overlay is a safe action that can never accidentally quit
/// the app" is exactly the property ADR 0020 asks this feature to hold.
///
/// `app.jump_popup()` (ADR 0022) is the next short-circuit, mirroring the
/// help overlay's own structure: while the jump-target popup is open,
/// `j`/`k`/Up/Down move its own selection, Enter confirms (`PopupConfirm`),
/// Esc cancels (`PopupCancel`), and every other key is swallowed.
///
/// `app.update_prompt_open()` (ADR 0054) is the next short-circuit: Enter
/// confirms (`PopupConfirm`), Esc/`q` cancel (`PopupCancel`), and every
/// other key is swallowed — the same shape as the jump popup above, minus
/// its own Up/Down selection (this popup has none).
///
/// `app.pending_prefix()` (ADR 0022) is consulted only for `d`/`r`: when a
/// `g` press is still pending, `d` resolves to `GotoDefinition` and `r` to
/// `GotoReferences` instead of their own ordinary meanings (`ToggleDiff`/
/// unbound) — every other key falls through to its normal translation
/// unconditionally, which is what lets the pending prefix's own state
/// (`App::handle_key`'s blanket clear-unless-`PendingGoto` rule) correctly
/// unwind on any key that is not `d`/`r`.
pub(crate) fn translate_key(code: KeyCode, modifiers: KeyModifiers, app: &App) -> Option<InputKey> {
    // The review overlay (ADR 0048) is checked before even the help
    // overlay: while composing a note, every printable character the
    // reviewer types (including `?`) must land in the note buffer, not
    // trigger the help overlay or any other single-key gesture. Composing
    // is also the one mode exempt from full-width normalization below —
    // free text must keep whatever the reviewer actually typed.
    if let review::ReviewMode::Compose { .. } = app.review().mode() {
        return match code {
            KeyCode::Enter => Some(InputKey::PopupConfirm),
            KeyCode::Esc => Some(InputKey::PopupCancel),
            KeyCode::Backspace => Some(InputKey::ComposeBackspace),
            KeyCode::Char(c) => Some(InputKey::ComposeChar(c)),
            _ => None,
        };
    }
    let code = normalize_fullwidth_key(code);
    match app.review().mode() {
        review::ReviewMode::List { .. }
        | review::ReviewMode::ExportMenu { .. }
        | review::ReviewMode::VerdictMenu { .. } => {
            return match code {
                KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
                KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
                KeyCode::Enter => Some(InputKey::PopupConfirm),
                KeyCode::Esc | KeyCode::Char('q') => Some(InputKey::PopupCancel),
                KeyCode::Char('d') => Some(InputKey::NoteDelete),
                _ => None,
            };
        }
        review::ReviewMode::Compose { .. } => unreachable!("handled by the early return above"),
        review::ReviewMode::Idle => {}
    }

    // Search composing (ADR 0057) is checked next, mirroring the review
    // overlay's own early-return shape just above: while composing a
    // query, every printable character (including keys that would
    // otherwise mean something else, like `?`) must land in the query
    // buffer. Only reachable on `Screen::Source` — `InputKey::SearchStart`
    // is likewise only ever emitted there (below), so this branch is
    // unreachable on `Screen::Entry` in practice, but gated on
    // `on_source_screen` defensively rather than relying on that
    // invariant, the same defensive style `FocusLeft`'s Esc arm below
    // already uses.
    let on_source_screen = matches!(app.screen(), Screen::Source { .. });
    if on_source_screen && matches!(app.search().mode(), SearchMode::Composing { .. }) {
        return match code {
            KeyCode::Enter => Some(InputKey::SearchConfirm),
            KeyCode::Esc => Some(InputKey::SearchCancel),
            KeyCode::Backspace => Some(InputKey::SearchBackspace),
            KeyCode::Char(c) => Some(InputKey::SearchChar(c)),
            _ => None,
        };
    }

    if app.help_open() {
        // The overlay's own content can run longer than its box (this
        // feature's whole reason for existing) — `j`/`k`/`Ctrl-d`/`Ctrl-u`/
        // `G` scroll it, mirroring the plain-key mapping each already has
        // outside the overlay so a reviewer does not have to learn a
        // second gesture just because the overlay is open. `gg`'s
        // second-`g` resolution still goes through the `pending_prefix`
        // branch below (this early return only covers `?`/Esc/`q`/`Ctrl-d`/
        // `Ctrl-u`/`G` and the bare `j`/`k`/arrow keys; a first `g` press
        // is deliberately *not* matched here so it falls through to the
        // ordinary `PendingGoto` arm at the bottom of this function, which
        // works identically whether the overlay is open or not since it
        // only touches `app.pending_prefix()`).
        return match code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => Some(InputKey::ToggleHelp),
            KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
            KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputKey::ScrollHalfPageDown)
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                Some(InputKey::ScrollHalfPageUp)
            }
            KeyCode::Char('G') => Some(InputKey::ScrollToBottom),
            KeyCode::Char('g') if app.pending_prefix() == Some(app::PendingPrefix::G) => {
                Some(InputKey::ScrollToTop)
            }
            KeyCode::Char('g') => Some(InputKey::PendingGoto),
            _ => None,
        };
    }

    if app.jump_popup().is_some() {
        return match code {
            KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
            KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
            KeyCode::Enter => Some(InputKey::PopupConfirm),
            KeyCode::Esc => Some(InputKey::PopupCancel),
            _ => None,
        };
    }

    // Without this short-circuit, Enter/Esc/`q` fell through to their
    // ordinary entry-view meanings, none of which `App::handle_key`'s
    // `update_prompt_open` branch recognizes as confirm/cancel (ADR 0056).
    if app.update_prompt_open() {
        return match code {
            KeyCode::Enter => Some(InputKey::PopupConfirm),
            KeyCode::Esc | KeyCode::Char('q') => Some(InputKey::PopupCancel),
            _ => None,
        };
    }

    let right_focused = app.focus() == app::Focus::Right;

    if app.pending_prefix() == Some(app::PendingPrefix::G) {
        match code {
            KeyCode::Char('d') => return Some(InputKey::GotoDefinition),
            KeyCode::Char('r') => return Some(InputKey::GotoReferences),
            // `gg` (ADR 0026): scroll the reading pane to the top —
            // resolved here the same way `gd`/`gr` are, piggybacking on
            // the existing `g`-prefix state machine (ADR 0022) rather
            // than reserving single-key `g` for this and breaking the
            // two-key sequences above. Uppercase `G` is a *distinct*
            // single-key gesture (`ScrollToBottom` below), unrelated to
            // this prefix — a second `g` in this arm is what means "top".
            KeyCode::Char('g') => return Some(InputKey::ScrollToTop),
            _ => {}
        }
    }

    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
        // Space always means "expand/collapse", never "drill in" — kept
        // distinct from Enter's own `InputKey::Open` (ADR 0020) so Space on
        // a file/symbol row never moves focus. Translated unconditionally
        // here regardless of `app.focus()`, same as every other key this
        // function maps context-free — `App::handle_key`'s own
        // `Focus::Tree`-only arm for `Select` is where the actual
        // Tree-focus requirement lives (mirroring how `NextHunk`/`PrevHunk`
        // are also translated unconditionally but only acted on under
        // certain conditions elsewhere).
        KeyCode::Char(' ') => Some(InputKey::Select),
        KeyCode::Enter => Some(InputKey::Open),
        KeyCode::Char('e') => Some(InputKey::ExpandAll),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Some(InputKey::Quit),
        KeyCode::Char('c') => Some(InputKey::CollapseAll),
        KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => Some(InputKey::JumpBack),
        // Ctrl-I and Tab share the same control code (0x09) at the terminal
        // protocol level — without Kitty's keyboard-enhancement protocol
        // (which this crate does not enable), a real Ctrl-I keypress
        // arrives here as plain `KeyCode::Tab`, not `KeyCode::Char('i')` +
        // `CONTROL` (confirmed via manual tmux testing against a real
        // terminal, not just documentation: the `Char('i') + CONTROL` arm
        // alone never matched a real Ctrl-I press). Both patterns are kept
        // so this still works correctly in an environment that *does*
        // report the modifier form (e.g. a test harness constructing the
        // event directly, as this module's own tests do).
        KeyCode::Tab => Some(InputKey::JumpForward),
        KeyCode::Char('i') if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(InputKey::JumpForward)
        }
        KeyCode::Char('o') => Some(InputKey::ToggleOrder),
        // `Ctrl-d`/`Ctrl-u` (ADR 0026): half-page scroll on the reading
        // pane (`Screen::Source`, or `Screen::Entry` + `Focus::Right`).
        // Must come *before* the plain `Char('d')`/`Char('u')` arms —
        // otherwise a `Ctrl-d` press would match `ToggleDiff` first and
        // the modifier would be ignored, silently rebinding "half-page
        // down" to "toggle diff pane". Emitted regardless of screen/
        // focus; `App::handle_scroll_key` no-ops on `Focus::Tree` in the
        // entry view (ADR 0026 decision 3's Tree-focus rule).
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(InputKey::ScrollHalfPageDown)
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            Some(InputKey::ScrollHalfPageUp)
        }
        KeyCode::Char('d') => Some(InputKey::ToggleDiff),
        KeyCode::Char('r') => Some(InputKey::ToggleBlastRadius),
        KeyCode::Char('v') => Some(InputKey::ToggleSplitView),
        // `G` (`Shift-g`, ADR 0026): scroll to the bottom. Distinct from
        // single-key lowercase `g` (`PendingGoto` below), which is the
        // leading key of the `gd`/`gr`/`gg` two-key sequences resolved
        // at the top of this function.
        KeyCode::Char('G') => Some(InputKey::ScrollToBottom),
        // `h`, or Esc while the right pane has focus: return focus to the
        // tree (ADR 0020's neovim-style "move left/back"). Checked before
        // the source-screen Esc arm below so `h`/Esc while Right-focused
        // never reaches the source screen (impossible in practice today,
        // since opening the source screen already moves focus to `Right`,
        // but ordered defensively rather than relying on that invariant).
        KeyCode::Char('h') if right_focused => Some(InputKey::FocusLeft),
        KeyCode::Esc if right_focused && !on_source_screen => Some(InputKey::FocusLeft),
        // `]c`/`[c` (vim's hunk-jump idiom) are read here as a single
        // bracket keystroke rather than a buffered two-key chord — this
        // crate's event loop (`run_app`) has no notion of a pending-chord
        // state machine today, and introducing one for exactly one binding
        // would be disproportionate; `]`/`[` alone are otherwise unbound,
        // so no existing gesture is lost by this simplification.
        KeyCode::Char(']') => Some(InputKey::NextHunk),
        KeyCode::Char('[') => Some(InputKey::PrevHunk),
        KeyCode::Char('s') => Some(InputKey::Source),
        // `/` (ADR 0057): starts composing a Source-view search query.
        // Source-screen-only — unbound on the entry screen, where a diff
        // pane search is future work (ADR 0057's own Alternatives).
        KeyCode::Char('/') if on_source_screen => Some(InputKey::SearchStart),
        // `n`/`N` (ADR 0057): jump to the next/previous search match.
        // Source-screen-only, checked ahead of the entry-screen `n`/`N`
        // arms just below so the two never collide — a search never has
        // more than [`SearchState::matches`] to navigate on this screen,
        // and the entry screen's review-note `n`/`N` (ADR 0048) are
        // unaffected since this arm never matches there.
        KeyCode::Char('n') if on_source_screen => Some(InputKey::SearchNext),
        KeyCode::Char('N') if on_source_screen => Some(InputKey::SearchPrev),
        // `n` (ADR 0048): opens the review-note compose overlay over the
        // row under the cursor. `N`: opens the review-notes list overlay.
        // Both are only meaningful on the entry screen (Source-screen
        // rows have no `SelectionSnapshot` to compose against, and are
        // shadowed by the search bindings just above there anyway).
        KeyCode::Char('n') => Some(InputKey::NoteCompose),
        KeyCode::Char('N') => Some(InputKey::NotesList),
        // `w` (ADR 0050): opens the current PR's page in a web browser —
        // matches `gh` CLI's own `-w`/`--web` convention. Global regardless
        // of screen/focus, like `d`/`r`/`s`; `crate::lib::run_app`
        // special-cases the actual dispatch (it needs the session's
        // `PrContext`, which `App` doesn't hold).
        KeyCode::Char('w') => Some(InputKey::OpenPrInBrowser),
        // `u` (ADR 0054): opens the update confirmation popup. Global,
        // like `w`/`d`/`r`/`s`; `App::handle_key`'s own arm no-ops unless
        // `App::update_available` is `Some`.
        KeyCode::Char('u') => Some(InputKey::OpenUpdatePrompt),
        // `g` (ADR 0022): the first half of the `gd`/`gr` two-key sequence.
        // Checked after the `pending_prefix` resolution above so a second
        // `g` press (`gg`, not a bound sequence today) simply restarts the
        // pending state rather than doing anything else — `App::handle_key`
        // sets `pending_prefix` from this variant unconditionally.
        KeyCode::Char('g') => Some(InputKey::PendingGoto),
        KeyCode::Char('?') => Some(InputKey::ToggleHelp),
        // ADR 0057: Esc's first press clears an active confirmed search
        // (matching vim's own `/`-then-Esc convention of dismissing the
        // highlight before leaving) rather than immediately returning to
        // the entry view — checked ahead of the plain `Back` arm just
        // below so a reviewer backing out of a search does not also lose
        // their place in the file.
        KeyCode::Esc if on_source_screen && app.search().query().is_some() => {
            Some(InputKey::SearchCancel)
        }
        KeyCode::Esc if on_source_screen => Some(InputKey::Back),
        KeyCode::Char('q') if on_source_screen => Some(InputKey::Back),
        KeyCode::Char('q') => Some(InputKey::Quit),
        _ => None,
    }
}

/// Folds a full-width form (U+FF01-U+FF5E, the Unicode "Fullwidth ASCII
/// Variants" block a Japanese/CJK IME sends when left on while a reviewer
/// presses an otherwise-ASCII binding) down to its ordinary half-width
/// `KeyCode::Char`, leaving every other `KeyCode` untouched. Applied to
/// every normal-mode/overlay gesture in [`translate_key`] but not while
/// [`review::ReviewMode::Compose`] is open — that buffer is free text, so a
/// full-width character typed there must reach the note body unchanged.
fn normalize_fullwidth_key(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(c @ '\u{FF01}'..='\u{FF5E}') => {
            KeyCode::Char(char::from_u32(c as u32 - 0xFEE0).unwrap_or(c))
        }
        other => other,
    }
}

/// Translates a raw `crossterm` mouse event into an [`InputKey`], the same
/// boundary role [`translate_key`] plays for keyboard input — a pure
/// function so the mapping is unit-testable without a live terminal.
///
/// Only `ScrollUp`/`ScrollDown` (wheel/trackpad) are mapped, and they are
/// mapped onto the *existing* [`InputKey::Up`]/[`InputKey::Down`] variants
/// rather than a dedicated pair of scroll variants: `App::handle_key`
/// already gives `Up`/`Down` the right contextual meaning everywhere a
/// wheel scroll should act — the tree cursor while [`app::Focus::Tree`],
/// [`app::App::right_pane_scroll`] by one line while [`app::Focus::Right`]
/// (ADR 0020), and [`app::Screen::Source`]'s `scroll_top` on the source
/// screen (ADR 0026) — so reusing them is a strict simplification (no new
/// state-machine surface) rather than introducing a second, parallel
/// motion concept the app would have to keep in sync with the first.
///
/// `MouseEventKind::ScrollLeft`/`ScrollRight` (horizontal wheel/trackpad)
/// and every click/drag/move variant are deliberately unmapped (`None`):
/// this crate has no horizontally-scrollable pane, and no pane targeting by
/// click position — the row/column the event occurred at is intentionally
/// not consulted here. Wheel input always acts on whichever pane already
/// has focus, exactly like a keyboard `j`/`k` press would; teaching the
/// wheel to also *change* focus by clicking a pane is future scope, not
/// attempted by this function.
pub(crate) fn translate_mouse_event(kind: event::MouseEventKind) -> Option<InputKey> {
    match kind {
        event::MouseEventKind::ScrollUp => Some(InputKey::Up),
        event::MouseEventKind::ScrollDown => Some(InputKey::Down),
        _ => None,
    }
}

#[cfg(test)]
#[path = "input_translate_tests/mod.rs"]
mod tests;
