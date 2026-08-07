//! The jump-target popup and jumplist (ADR 0022), split out of
//! `app/mod.rs` (ADR 0028): the popup/jumplist types, [`App`]'s own
//! `jump_to_symbol`/`sync_tree_cursor_to_symbol`/`open_jump_popup` methods,
//! and `handle_key_with_popup_open` (the popup's own `Up`/`Down`/
//! `PopupConfirm`/`PopupCancel` dispatch, moved here from
//! `app/handle_key.rs` since it acts on this module's own `jump_popup`
//! state). The `g`-prefix bookkeeping that *opens* the popup stays in
//! `app/handle_key.rs`, which calls into this module's methods once a
//! candidate is resolved.

use crate::detail::SymbolMention;

use super::{App, InputKey};

/// A `g`-prefixed two-key sequence awaiting its second key (ADR 0022's
/// minimal prefix state machine — not a general chord engine, see that
/// ADR's own Alternatives). Today's only prefix is `g`; the variant exists
/// so `App`'s `pending_prefix` field reads as "which prefix, if any" rather
/// than a bare `bool` that would only ever mean "g was just pressed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingPrefix {
    G,
}

/// One candidate in the jump-target popup (ADR 0022) — the same identity
/// [`SymbolMention`] already carries, kept as a separate type rather than
/// reusing `SymbolMention` directly so the popup's own view-model is not
/// coupled to the Detail pane's type if the two ever need to diverge (e.g.
/// the popup later gaining a fan-in count `SymbolMention` doesn't carry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpCandidate {
    pub id: String,
    pub name: String,
    pub path: String,
}

impl From<&SymbolMention> for JumpCandidate {
    fn from(mention: &SymbolMention) -> Self {
        Self {
            id: mention.id.clone(),
            name: mention.name.clone(),
            path: mention.path.clone(),
        }
    }
}

/// The jump-target popup's state (ADR 0022) while it is open: every
/// candidate found for the pending `gd`/`gr` press, plus which one the
/// popup's own `j`/`k` cursor currently highlights. Mirrors `help_open`'s
/// flag-not-`Screen` design (`App::help_open`'s own doc comment) for the
/// same reason: the popup sits on top of whatever was already showing and
/// closing it (via `PopupConfirm` or `PopupCancel`) must not disturb that
/// underlying state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpPopup {
    pub candidates: Vec<JumpCandidate>,
    pub cursor: usize,
}

/// One jumplist entry (ADR 0022): just enough state to restore "what the
/// reviewer was looking at" — the symbol and the right pane's scroll offset
/// into it — deliberately not a full `App` snapshot (see the ADR's own
/// Alternatives on why a full snapshot was rejected).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JumplistEntry {
    pub(super) symbol_id: String,
    pub(super) right_pane_scroll: usize,
}

/// The jumplist's cap (ADR 0022 decision 4): oldest entries are dropped
/// once the back-stack would exceed this, since a reviewing session
/// realistically never needs more and an unbounded stack is an unnecessary
/// unbounded-growth risk for a long-running TUI session.
pub(super) const JUMPLIST_CAP: usize = 100;

impl App {
    /// Jumps the cursor to `symbol_id` (ADR 0022): pushes the *current*
    /// location onto the jumplist's back-stack (capped at
    /// [`JUMPLIST_CAP`], oldest dropped) and clears the forward-stack (a new
    /// jump abandons any history the reviewer had already jumped back past
    /// — vim's own jumplist does the same), then moves the tree cursor via
    /// [`crate::nav::Nav::move_cursor_to_symbol`] (expanding collapsed
    /// ancestors) and resets the scroll offset to 0 so the jumped-to
    /// symbol's content starts from its top. Focus is deliberately left
    /// untouched (ADR 0022's own "keep reading" rationale).
    ///
    /// The jumplist push only happens when the cursor was already on a
    /// present symbol row (`Self::selected_symbol_id`) — every real caller
    /// (`crate::run_app`'s `resolve_goto`/`GotoOutcome` handling, and this
    /// method's own popup-confirm caller in `Self::handle_key_with_popup_open`)
    /// only reaches this method after confirming that already, per ADR
    /// 0022's "only a symbol row is a valid jump source" rule, so this is a
    /// defensive fallback (silently skip recording jumplist history) rather
    /// than a precondition that blocks the jump itself — the cursor still
    /// moves either way, since refusing to jump at all over a bookkeeping
    /// detail would be a worse failure mode than an incomplete jumplist.
    ///
    /// A no-op (with a status message), without touching the jumplist, when
    /// no row's symbol id matches `symbol_id` (defensive: callers are
    /// expected to have already confirmed the id exists via
    /// `crate::detail::symbol_mentions`, but `App` does not trust that
    /// blindly).
    ///
    /// A successful jump also cancels any confirmed tree search: expanding
    /// collapsed ancestors reshapes the visible row list a confirmed
    /// search's frozen row indices were built against — the same invariant
    /// `Self::handle_key`'s `Select`/`ExpandAll`/`CollapseAll`/`ToggleOrder`
    /// arms enforce (ADR 0057 decision 2's "cancel means stop searching
    /// altogether"). The failed-jump early return above deliberately keeps
    /// the search: nothing moved, so the frozen indices are still valid.
    pub fn jump_to_symbol(mut self, symbol_id: &str) -> Self {
        let current_id = self.selected_symbol_id().map(str::to_string);

        let mut nav = self.nav.clone();
        if !nav.move_cursor_to_symbol(&self.tree, symbol_id) {
            self.status = Some(format!("note: symbol {symbol_id} is no longer present"));
            return self;
        }

        if let Some(current_id) = current_id {
            self.push_jumplist_entry(JumplistEntry {
                symbol_id: current_id,
                right_pane_scroll: self.right_pane_scroll,
            });
            self.jump_forward.clear();
        }
        self.nav = nav;
        self.right_pane_scroll = 0;
        self.search = self.search.clone().cancel();
        self
    }

    /// Moves the tree cursor to `symbol_id` (ADR 0030: manual diff-pane
    /// scrolling syncs the cursor back to the visible symbol), expanding
    /// collapsed ancestors on the way via
    /// [`crate::nav::Nav::move_cursor_to_symbol`] — same underlying move
    /// [`Self::jump_to_symbol`] performs, but deliberately *not* that
    /// method: this sync must not push a jumplist entry (a scroll session
    /// through several symbols would otherwise flood `Ctrl-o`/`Ctrl-i`'s
    /// history with moves the reviewer never asked to navigate through —
    /// ADR 0022's jumplist is for explicit `gd`/`gr` jumps only) and must
    /// not reset [`super::App::right_pane_scroll`] (the scroll offset that
    /// just triggered this sync is exactly the value the caller wants
    /// preserved — resetting it here would make the sync fight its own
    /// trigger). A no-op (returning `self` unchanged, no status message —
    /// unlike `jump_to_symbol`'s, since a missing symbol here is an
    /// ordinary transient case, e.g. mid-recompute after a report reload,
    /// not a reviewer-facing navigation failure) when no row's symbol id
    /// matches `symbol_id`.
    pub fn sync_tree_cursor_to_symbol(mut self, symbol_id: &str) -> Self {
        let mut nav = self.nav.clone();
        if nav.move_cursor_to_symbol(&self.tree, symbol_id) {
            self.nav = nav;
        }
        self
    }

    /// Opens the jump-target popup (ADR 0022) over `candidates` — called by
    /// `crate::run_app` once it has resolved more than one candidate for a
    /// pending `gd`/`gr` (`InputKey::GotoDefinition`/`GotoReferences`'s own
    /// doc comment on why resolution happens there, not in `App`).
    pub fn open_jump_popup(mut self, candidates: Vec<JumpCandidate>) -> Self {
        self.jump_popup = Some(JumpPopup {
            candidates,
            cursor: 0,
        });
        self
    }

    /// Pushes `entry` onto the jumplist's back-stack, dropping the oldest
    /// entry first if this would exceed [`JUMPLIST_CAP`].
    pub(super) fn push_jumplist_entry(&mut self, entry: JumplistEntry) {
        if self.jump_back.len() >= JUMPLIST_CAP {
            // `Vec::remove(0)` is O(n) (shifts every remaining element down
            // one slot) rather than O(1) — a `VecDeque` would make this
            // O(1), but at `JUMPLIST_CAP` = 100 small (`String` + `usize`)
            // entries, shifting is at most ~100 pointer-sized moves, only
            // once per jump and only once the cap is already full (every
            // jump before that is a plain `push`, already O(1)) — not a
            // measurable cost against a single keypress in an interactive
            // TUI. `Vec` also keeps this consistent with `jump_forward`
            // (`Vec<JumplistEntry>` too) without introducing a second
            // container type for one already-negligible operation.
            self.jump_back.remove(0);
        }
        self.jump_back.push(entry);
    }

    /// Handles one [`InputKey`] while the jump-target popup (ADR 0022) is
    /// open — mirrors the help overlay's own "takes over the whole key
    /// space" structure (`Self::handle_key`'s own doc comment): `Up`/`Down`
    /// move the popup's own selection cursor (clamped, not wrapping, same
    /// convention `Nav::handle`'s `CursorUp`/`CursorDown` already use),
    /// `PopupConfirm` jumps to the highlighted candidate and closes the
    /// popup, `PopupCancel` closes it without jumping, and every other key
    /// is swallowed as a no-op.
    pub(super) fn handle_key_with_popup_open(mut self, key: InputKey) -> Self {
        let Some(popup) = self.jump_popup.clone() else {
            // Unreachable: this method is only called from `Self::handle_key`
            // when `self.jump_popup.is_some()`.
            return self;
        };

        match key {
            InputKey::Up => {
                if let Some(popup) = &mut self.jump_popup {
                    popup.cursor = popup.cursor.saturating_sub(1);
                }
            }
            InputKey::Down => {
                if let Some(popup) = &mut self.jump_popup {
                    popup.cursor = (popup.cursor + 1).min(popup.candidates.len().saturating_sub(1));
                }
            }
            InputKey::PopupConfirm => {
                let target = popup.candidates.get(popup.cursor).map(|c| c.id.clone());
                self.jump_popup = None;
                if let Some(target) = target {
                    self = self.jump_to_symbol(&target);
                }
            }
            InputKey::PopupCancel => {
                self.jump_popup = None;
            }
            _ => {}
        }

        self
    }
}
