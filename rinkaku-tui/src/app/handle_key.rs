//! Key-event dispatch: [`App::handle_key`] and [`App::handle_scroll_key`],
//! split out of `app/mod.rs` (ADR 0028) since the two together are the
//! single largest responsibility on `App` — routing every [`InputKey`]
//! against `Screen`/`Focus` state. Struct/enum definitions and state
//! accessors stay in `app/mod.rs`; only the dispatch logic lives here.

use super::{App, Focus, InputKey, JumplistEntry, PendingPrefix, RightPane, Screen};
use crate::nav::Action;
use crate::order::OrderMode;
use crate::tree::NodeKind;

impl App {
    /// Applies one [`InputKey`] and returns the next `App`. `report` is
    /// needed only for [`InputKey::Source`] (to confirm the row under the
    /// cursor is a present symbol before switching screens — the actual
    /// file read happens later, in `crate::run`, once `Screen::Source` is
    /// active) and is otherwise unused.
    ///
    /// The `?` help overlay (ADR 0020) is handled first and takes over the
    /// whole key space while open: `ToggleHelp` closes it, `Up`/`Down` move
    /// [`Self::help_scroll`] by one line (the content can run longer than
    /// the overlay's own box — ADR 0026's follow-up, this struct's own
    /// `help_scroll` doc comment), `ScrollHalfPageDown`/`ScrollHalfPageUp`/
    /// `ScrollToTop`/`ScrollToBottom` are let through unmodified for
    /// [`Self::handle_scroll_key`] to apply (mirroring ADR 0026's existing
    /// two-step dispatch for the source screen/right pane — see that
    /// method's own doc comment), and every other key is swallowed as a
    /// no-op (deliberately, including `Quit` — the overlay's whole point is
    /// a safe, low-stakes "let me check the keys" action that cannot be
    /// short-circuited by an accidental app exit; see `Self::help_open`'s
    /// own doc comment). This must run before the screen/focus dispatch
    /// below, not as another arm inside it, so no future `InputKey` variant
    /// can accidentally bypass the overlay by being handled in a
    /// screen-specific branch first.
    ///
    /// `right_pane_scroll` is reset to 0 by every key *except* `Up`/`Down`
    /// while [`Focus::Right`] on [`Screen::Entry`] (ADR 0020: scrolling
    /// moved onto the same physical keys as cursor movement, gated by focus
    /// rather than a separate uppercase pair) — a uniform rule applied once
    /// below, rather than each action deciding individually whether it
    /// might change the right pane's content. The per-action approach used
    /// to miss cases where the cursor moves *indirectly*: collapsing a
    /// directory (`Select`/`CollapseAll`) can retarget the cursor onto a
    /// different row via `Nav::retarget_cursor`, and reordering
    /// (`ToggleOrder`) can do the same simply by changing which row now
    /// sits at the same cursor index — both used to leave a stale nonzero
    /// scroll offset pointing into the *new* row's unrelated content.
    ///
    /// The jump-target popup (ADR 0022) is handled next, mirroring the help
    /// overlay's own "takes over the whole key space while open" structure:
    /// `Up`/`Down` move the popup's own selection, `PopupConfirm` jumps to
    /// the highlighted candidate and closes it, `PopupCancel` closes it
    /// without jumping, and every other key is swallowed. This runs before
    /// the `g`-prefix bookkeeping and the screen/focus dispatch below for
    /// the same "no future variant can accidentally bypass it" reason the
    /// help overlay's own check does.
    ///
    /// `pending_prefix` (ADR 0022's minimal `g`-prefix state machine) is
    /// cleared by every key except [`InputKey::PendingGoto`] itself, which
    /// sets it — a blanket rule applied *unconditionally at the very top of
    /// this function*, before the `help_open`/`jump_popup` early returns
    /// below, rather than alongside the scroll-reset rule further down
    /// (post-review finding: clearing it after those early returns let a
    /// pending prefix survive both of them — the help overlay's own early
    /// return, and more importantly the jump-popup one, since opening the
    /// popup is itself the direct result of a `gd`/`gr` press that
    /// `crate::run_app` resolves *without ever calling this method at all*,
    /// see that resolution's own doc comment — so the clear could not
    /// safely live only "later in the normal path" the way scroll-reset
    /// does; it has to run before anything can return early). Every key
    /// this method is ever called with, from every call site, hits this one
    /// line before doing anything else, so a `g` press followed by anything
    /// other than `d`/`r` (already resolved by `crate::lib::translate_key`
    /// before this function ever sees the key) cannot leave a stale pending
    /// prefix behind regardless of which branch handles the key afterward.
    pub fn handle_key(mut self, key: InputKey) -> Self {
        self.status = None;
        self.pending_prefix = if key == InputKey::PendingGoto {
            Some(PendingPrefix::G)
        } else {
            None
        };

        if self.help_open {
            match key {
                InputKey::ToggleHelp => {
                    self.help_open = false;
                    self.help_scroll = 0;
                }
                InputKey::Down => {
                    self.help_scroll = self.help_scroll.saturating_add(1);
                }
                InputKey::Up => {
                    self.help_scroll = self.help_scroll.saturating_sub(1);
                }
                // The four ADR 0026 scroll variants are deliberately not
                // handled here: their step size depends on the overlay's
                // rendered height, which only `Self::handle_scroll_key`
                // receives (`crate::run_app`'s two-step dispatch) — passed
                // through as a no-op here so that call still runs (matching
                // every other scroll key's own `handle_key` arm, ADR 0026's
                // `is_scroll_input_key` doc comment).
                InputKey::ScrollHalfPageDown
                | InputKey::ScrollHalfPageUp
                | InputKey::ScrollToTop
                | InputKey::ScrollToBottom => {}
                _ => {}
            }
            return self;
        }
        if key == InputKey::ToggleHelp {
            self.help_open = true;
            self.help_scroll = 0;
            return self;
        }

        if self.jump_popup.is_some() {
            return self.handle_key_with_popup_open(key);
        }

        let preserve_scroll = matches!(
            (&self.screen, self.focus, key),
            (Screen::Entry, Focus::Right, InputKey::Up)
                | (Screen::Entry, Focus::Right, InputKey::Down)
                // ADR 0026: half-page and top/bottom keys are dispatched
                // through `handle_scroll_key` after this method returns
                // (`crate::run_app`'s two-step dispatch). Without this
                // exception, the blanket "reset scroll to 0" at the end
                // of this function would wipe the scroll offset a moment
                // before `handle_scroll_key` overwrote it — same class
                // of bug the `Up`/`Down` case above already exists to
                // prevent for plain j/k scrolling.
                | (Screen::Entry, Focus::Right, InputKey::ScrollHalfPageDown)
                | (Screen::Entry, Focus::Right, InputKey::ScrollHalfPageUp)
                | (Screen::Entry, Focus::Right, InputKey::ScrollToTop)
                | (Screen::Entry, Focus::Right, InputKey::ScrollToBottom)
                // Independent-review finding: `crate::run_app`'s
                // `dispatch_non_source_key` always calls `handle_key(input_key)`
                // for `GotoDefinition`/`GotoReferences` first (ADR 0022's
                // `pending_prefix`-clear requirement, this arm's own doc
                // comment below), *before* `resolve_goto`/`Self::jump_to_symbol`
                // ever runs. Without this case, that first call hit the
                // blanket reset at the bottom of this function and zeroed
                // `right_pane_scroll` *before* `jump_to_symbol` read it to
                // record the jumplist entry the reviewer is jumping *from* —
                // so every `Ctrl-o` (`InputKey::JumpBack`) landed back at
                // scroll 0 regardless of how far down the reviewer had
                // actually scrolled. `InputKey::PendingGoto` (the leading
                // `g` of the two-key `gd`/`gr` sequence) needs the same
                // exception for the identical reason: it is a real,
                // separately-dispatched `handle_key` call in the actual
                // `crate::run_app` event loop (unlike a test calling
                // `dispatch_non_source_key(..., GotoDefinition)` directly,
                // which skips straight past this — the gap an earlier
                // version of this fix's own regression test had, caught only
                // by a real terminal run), so scroll was already zeroed by
                // the `g` press itself, one key before `GotoDefinition`/
                // `GotoReferences` even had a chance to matter.
                // `Self::jump_to_symbol` (and
                // `Self::handle_key_with_popup_open`'s `PopupConfirm`, which
                // calls it too) already does its own correct
                // `self.right_pane_scroll = 0` reset for the *new* target
                // once the jump actually happens, so preserving scroll
                // through this first call only protects the jumplist
                // snapshot of the *old* position — it does not defeat the
                // reset a real jump still performs.
                | (Screen::Entry, _, InputKey::PendingGoto)
                | (Screen::Entry, _, InputKey::GotoDefinition)
                | (Screen::Entry, _, InputKey::GotoReferences)
        ) || matches!(
            (&self.screen, self.focus, self.right_pane, key),
            // Map-assisted-review finding (`InputKey::Open`'s own doc
            // comment): Enter pressed again while already Right-focused on
            // an already-showing Diff pane is a complete no-op — reading
            // position must survive it, the same way plain scrolling does
            // just above. Gated on `right_pane` too (not just `screen`/
            // `focus`/`key`, unlike the `Up`/`Down` case above), since the
            // *other* `Focus::Right` case — Detail/BlastRadius showing —
            // is a real pane switch to Diff and must still fall through to
            // the blanket reset below.
            (Screen::Entry, Focus::Right, RightPane::Diff, InputKey::Open)
                // ADR 0027 dogfooding finding: Enter from Tree focus onto a
                // file/symbol row already showing Diff does not visibly
                // change the pane's content (Diff was already the current
                // right pane, and the auto-scroll for the row already ran
                // when the cursor first landed on it) — it just moves
                // focus. Resetting `right_pane_scroll` to 0 here would
                // wipe both the auto-scroll offset and any subsequent
                // manual scrolling the reviewer did before pressing Enter,
                // dropping them back at the top of the file instead of
                // the section they were reading. The other `Focus::Tree,
                // Open` cases — a directory row (expand/collapse only, no
                // focus change, dispatched via `Action::ToggleExpand`
                // below) and the Detail/BlastRadius→Diff swap — do change
                // the pane's content and must still fall through to the
                // reset.
                | (Screen::Entry, Focus::Tree, RightPane::Diff, InputKey::Open)
        );
        // Set by the `JumpBack`/`JumpForward` arms below when they actually
        // restore a jumplist entry's own scroll offset — that restored
        // value must survive the blanket "reset scroll to 0" rule at the
        // bottom of this function the same way `preserve_scroll` above lets
        // right-focused `Up`/`Down` survive it, just via a second, separate
        // flag rather than folding jump-restoration into `preserve_scroll`'s
        // own `matches!` (which is keyed on `(screen, focus, key)` alone and
        // has no way to express "only when the jump actually succeeded").
        let mut preserve_scroll_after_jump = false;

        match (&self.screen, self.focus, key) {
            (Screen::Source { .. }, _, InputKey::Back) => {
                self.screen = Screen::Entry;
            }
            // ADR 0026: `j`/`k` scroll the source pane by one line — the
            // same "j/k scrolls the reading pane" rule ADR 0020 already
            // applies to the entry view's right pane, extended here so a
            // reviewer can read the caller a few lines above or the
            // helper a few lines below the symbol they drilled into,
            // rather than having to leave the TUI to see either.
            //
            // `scroll_top` is unclamped by design (see the field's own
            // doc comment): `crate::ui::draw_source_screen` clamps it
            // against the file's actual line count and the pane's
            // rendered height at draw time, matching how
            // `right_pane_scroll` already handles the same "App doesn't
            // know the layout" problem.
            (
                Screen::Source {
                    symbol_id,
                    scroll_top,
                },
                _,
                InputKey::Up,
            ) => {
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: scroll_top.saturating_sub(1),
                };
            }
            (
                Screen::Source {
                    symbol_id,
                    scroll_top,
                },
                _,
                InputKey::Down,
            ) => {
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: scroll_top.saturating_add(1),
                };
            }
            // Half-page steps and top/bottom jumps need the viewport
            // height (or a `usize::MAX` sentinel) that `App::handle_key`
            // doesn't know — they're handled by
            // [`App::handle_scroll_key`] instead. This arm is still
            // reached (via the ordinary `translate_key -> handle_key`
            // path) so the blanket `status = None` /
            // `pending_prefix = None` reset at the top of this function
            // runs on this path too; the actual state mutation happens
            // when `crate::run_app` calls `handle_scroll_key` next.
            (
                Screen::Source { .. },
                _,
                InputKey::ScrollHalfPageDown
                | InputKey::ScrollHalfPageUp
                | InputKey::ScrollToTop
                | InputKey::ScrollToBottom,
            ) => {}
            // Every other key is a no-op while the source view is open —
            // navigation/reordering only make sense against the entry
            // view's tree, and re-dispatching them would silently move
            // the cursor underneath a screen the user can't see moving.
            (Screen::Source { .. }, _, _) => {}

            (Screen::Entry, _, InputKey::Quit) => {
                self.should_quit = true;
            }
            (Screen::Entry, Focus::Tree, InputKey::Up) => {
                self.nav = self.nav.handle(Action::CursorUp, &self.tree);
            }
            (Screen::Entry, Focus::Tree, InputKey::Down) => {
                self.nav = self.nav.handle(Action::CursorDown, &self.tree);
            }
            (Screen::Entry, Focus::Right, InputKey::Up) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_sub(1);
            }
            (Screen::Entry, Focus::Right, InputKey::Down) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_add(1);
            }
            (Screen::Entry, Focus::Tree, InputKey::Select) => {
                self.nav = self.nav.handle(Action::ToggleExpand, &self.tree);
            }
            // Gated on `Focus::Tree`, matching `InputKey::Open`'s own focus
            // requirement (finding: Space used to fire regardless of focus,
            // inconsistent with Enter's own Tree-only reach for the same
            // "act on the row under the tree cursor" family of keys).
            // While `Focus::Right`, the tree cursor is always parked on
            // whichever file/symbol row is being previewed (only a
            // File/Symbol row's `Open` moves focus to `Right` at all, never
            // a `Dir` row's — see the `Open` arm below), so this can never
            // cut off a "collapse a directory while previewing its content"
            // workflow; there is no reachable state where the parked cursor
            // is a directory row here. What it *does* remove is Space
            // silently toggling that file/symbol row's own expand state
            // behind the currently-visible right pane — a change with no
            // visible effect until the user returns to `Focus::Tree`
            // (`h`/Esc), which is the kind of spooky-action-at-a-distance
            // this gate closes off.
            (Screen::Entry, Focus::Right, InputKey::Select) => {}
            // Map-assisted-review finding (`InputKey::Open`'s own doc
            // comment): while already `Focus::Right`, Enter no longer
            // re-derives "what row is this" from the tree cursor at all —
            // it only ever means "make sure Diff is showing". When Diff is
            // already showing this is a true no-op (nothing in `self`
            // changes, including `right_pane_scroll` — this arm plus
            // `preserve_scroll`'s matching `RightPane::Diff` case above are
            // what make that hold); when Detail/BlastRadius is showing, it
            // switches to Diff, matching `InputKey::ToggleDiff`'s own "from
            // Detail or BlastRadius, land on Diff" precedent just below.
            // Must come before the `(Screen::Entry, _, InputKey::Open)` arm
            // below (whose `Focus::Tree`-only tree-cursor dispatch this arm
            // deliberately bypasses) since match arms are tried in order.
            (Screen::Entry, Focus::Right, InputKey::Open) => {
                self.right_pane = RightPane::Diff;
            }
            (Screen::Entry, _, InputKey::Open) => {
                let rows = self.nav.rows(&self.tree);
                match rows.get(self.nav.cursor()).map(|row| &row.node.kind) {
                    // A directory row's Enter behaves exactly like Space
                    // (`InputKey::Select`) — expand/collapse, no focus
                    // change (ADR 0020: only a file/symbol row's Enter also
                    // drills in). A section row, a pure grouping node,
                    // behaves the same way.
                    Some(NodeKind::Dir) | Some(NodeKind::Section(_)) => {
                        self.nav = self.nav.handle(Action::ToggleExpand, &self.tree);
                    }
                    // File and symbol rows behave identically (dogfooding
                    // fix, `Self::Open`'s own doc comment): switch to the
                    // Diff pane and move focus there. A symbol row used to
                    // additionally jump into `Screen::Source` here, which
                    // read the file from the working tree and could fail —
                    // an asymmetric, occasionally-erroring behavior a file
                    // row's Enter never had. Enter is now always a pure,
                    // never-failing screen/pane transition regardless of row
                    // kind (including a removed symbol, which no longer
                    // needs its own guard here — this arm never touches the
                    // filesystem, unlike `InputKey::Source`'s own
                    // `!symbol_ref.removed` guard below, which still applies
                    // there since that key does open the source view).
                    Some(NodeKind::File) | Some(NodeKind::Symbol(_)) => {
                        self.right_pane = RightPane::Diff;
                        self.focus = Focus::Right;
                    }
                    None => {}
                }
            }
            (Screen::Entry, _, InputKey::ExpandAll) => {
                self.nav = self.nav.handle(Action::ExpandAll, &self.tree);
            }
            (Screen::Entry, _, InputKey::CollapseAll) => {
                self.nav = self.nav.handle(Action::CollapseAll, &self.tree);
            }
            (Screen::Entry, _, InputKey::ToggleOrder) => {
                self.order_mode = match self.order_mode {
                    OrderMode::Topological => OrderMode::AlphaNumeric,
                    OrderMode::AlphaNumeric => OrderMode::Topological,
                };
                crate::order::order_tree(&mut self.tree, &self.ranks, self.order_mode);
            }
            (Screen::Entry, _, InputKey::Source) => {
                let rows = self.nav.rows(&self.tree);
                if let Some(row) = rows.get(self.nav.cursor())
                    && let NodeKind::Symbol(symbol_ref) = &row.node.kind
                    && !symbol_ref.removed
                {
                    // `scroll_top` starts at 0 here; `crate::run_app`
                    // computes the centered start via `crate::source::
                    // visible_window` once it has the file loaded, then
                    // calls `Self::with_source_scroll_top` to overwrite
                    // this initial 0. Kept as two steps (`App` transitions
                    // to `Source`, then `run_app` back-fills the centered
                    // scroll) because `App::handle_key` has no access to
                    // the file's line count or the pane's rendered height
                    // — the same seam `run_app` already uses to feed
                    // `source_content` in after a file read.
                    self.screen = Screen::Source {
                        symbol_id: symbol_ref.id.clone(),
                        scroll_top: 0,
                    };
                }
            }
            (Screen::Entry, _, InputKey::ToggleDiff) => {
                self.right_pane = match self.right_pane {
                    RightPane::Diff => RightPane::Detail,
                    // From Detail or BlastRadius, `d` always lands on Diff —
                    // a deliberate, unconditional "go to Diff" gesture
                    // rather than consulting `blast_radius_return_pane`.
                    // Only `R`'s own re-press (`ToggleBlastRadius` below)
                    // restores the pre-blast-radius pane; `d` pressed while
                    // the blast-radius pane is active is its own
                    // independent choice of destination, matching this
                    // arm's existing "from Detail, `d` always lands on
                    // Diff" behavior rather than growing a second "restore
                    // the previous pane" rule that would only apply to
                    // some keys and not others.
                    RightPane::Detail | RightPane::BlastRadius => RightPane::Diff,
                };
            }
            (Screen::Entry, _, InputKey::ToggleBlastRadius) => {
                self.right_pane = match self.right_pane {
                    // Restore whichever pane was showing right before this
                    // blast-radius session started, rather than
                    // unconditionally Detail — `blast_radius_return_pane`
                    // was captured below the moment `BlastRadius` was
                    // entered, so e.g. `d` -> `R` -> `R` returns to Diff,
                    // not Detail.
                    RightPane::BlastRadius => self.blast_radius_return_pane,
                    RightPane::Detail | RightPane::Diff => {
                        self.blast_radius_return_pane = self.right_pane;
                        RightPane::BlastRadius
                    }
                };
            }
            (Screen::Entry, Focus::Right, InputKey::FocusLeft) => {
                self.focus = Focus::Tree;
            }
            (Screen::Entry, Focus::Tree, InputKey::FocusLeft) => {
                // No-op: nothing to return from — `h`/Esc while already
                // Tree-focused on the entry view has no target, mirroring
                // `InputKey::Back`'s own no-op arm on the entry screen.
            }
            // `]c`/`[c` hunk jumping is layout-dependent (it needs to know
            // where each hunk's shaped content actually starts once
            // wrapped to the pane's width), so `App` itself is always a
            // no-op here regardless of focus — the actual scroll-offset
            // jump is computed and applied in `crate::run_app`, the one
            // place both `App` and the shaped diff content
            // (`crate::diff_shape`) are in scope. `run_app` additionally
            // gates that jump on `Focus::Right` *and* `RightPane::Diff`
            // (not just `Focus::Right`, which is all this match can see) —
            // so pressing `]c` while Tree-focused, or while Right-focused
            // but viewing Detail/BlastRadius, is a no-op there too, rather than
            // scrolling those panes against a hunk-offset table computed
            // for the Diff pane.
            (Screen::Entry, Focus::Right, InputKey::NextHunk | InputKey::PrevHunk) => {}
            (Screen::Entry, Focus::Tree, InputKey::NextHunk | InputKey::PrevHunk) => {}
            (Screen::Entry, _, InputKey::Back) => {
                // No-op: Esc-as-back on the entry view while Tree-focused
                // has nowhere to return to (Focus::Right's own Esc meaning
                // is `InputKey::FocusLeft`, handled above — `crate::run`'s
                // `translate_key` maps Esc to `FocusLeft` while
                // `Focus::Right` and to `Back` only on the source screen, so
                // this arm is reached only defensively). Quitting from the
                // entry view is the dedicated `InputKey::Quit` variant
                // instead.
            }
            (Screen::Entry, _, InputKey::ToggleHelp) => {
                // Unreachable: handled above before this match, kept only
                // so the match stays exhaustive against future refactors.
            }
            (Screen::Entry, _, InputKey::PendingGoto) => {
                // No other state to change here: `pending_prefix` was
                // already set above, before this match, by the blanket rule
                // this function's own doc comment describes.
            }
            // `gd`/`gr` candidate resolution needs `report`, which this
            // function does not have — `crate::run_app` reads
            // `report.graph.edges` itself (`resolve_goto`) and applies the
            // outcome via `Self::jump_to_symbol`/`Self::open_jump_popup`/
            // `Self::set_status`. This arm is still reached, though: `run_app`
            // calls `handle_key(input_key)` for these two variants same as
            // every other key (post-review fix), specifically so the
            // unconditional `pending_prefix` clear at the top of this
            // function runs on this path too — before this fix, `run_app`
            // skipped `handle_key` entirely for `gd`/`gr`, leaving
            // `pending_prefix` stuck at `Some(G)` after every jump. The arm
            // itself stays a no-op: `resolve_goto`/the actual jump/popup/
            // status mutation happen in `run_app`, not here.
            (Screen::Entry, _, InputKey::GotoDefinition | InputKey::GotoReferences) => {}
            (Screen::Entry, _, InputKey::JumpBack) => {
                if let Some(entry) = self.jump_back.pop() {
                    let current_id = self.selected_symbol_id().map(str::to_string);
                    let mut nav = self.nav.clone();
                    if nav.move_cursor_to_symbol(&self.tree, &entry.symbol_id) {
                        if let Some(current_id) = current_id {
                            self.jump_forward.push(JumplistEntry {
                                symbol_id: current_id,
                                right_pane_scroll: self.right_pane_scroll,
                            });
                        }
                        self.nav = nav;
                        self.right_pane_scroll = entry.right_pane_scroll;
                        preserve_scroll_after_jump = true;
                    } else {
                        self.status = Some(format!(
                            "note: symbol {} is no longer present",
                            entry.symbol_id
                        ));
                    }
                } else {
                    self.status = Some("note: jumplist has no earlier location".to_string());
                }
            }
            (Screen::Entry, _, InputKey::JumpForward) => {
                if let Some(entry) = self.jump_forward.pop() {
                    let current_id = self.selected_symbol_id().map(str::to_string);
                    let mut nav = self.nav.clone();
                    if nav.move_cursor_to_symbol(&self.tree, &entry.symbol_id) {
                        if let Some(current_id) = current_id {
                            self.push_jumplist_entry(JumplistEntry {
                                symbol_id: current_id,
                                right_pane_scroll: self.right_pane_scroll,
                            });
                        }
                        self.nav = nav;
                        self.right_pane_scroll = entry.right_pane_scroll;
                        preserve_scroll_after_jump = true;
                    } else {
                        self.status = Some(format!(
                            "note: symbol {} is no longer present",
                            entry.symbol_id
                        ));
                    }
                } else {
                    self.status = Some("note: jumplist has no later location".to_string());
                }
            }
            // ADR 0026: half-page scroll and top/bottom jumps on the
            // entry view are handled by [`App::handle_scroll_key`] (which
            // has the viewport height) — same reasoning as the
            // corresponding `Screen::Source` arm above. This is deliberately
            // *not* gated on `Focus::Right`: `handle_scroll_key` itself
            // will no-op when Tree-focused, and having the gate live in
            // one place rather than duplicated here keeps the "which
            // focus/screen actually scrolls" answer discoverable in one
            // spot.
            //
            // On `Focus::Right`, the blanket end-of-function
            // `right_pane_scroll = 0` reset is skipped via `preserve_scroll`'s
            // exception list above, so `handle_scroll_key`'s subsequent write
            // is not silently wiped. On `Focus::Tree`, that exception does
            // *not* fire and the reset runs — which is fine because Tree
            // focus never has a right-pane scroll to preserve in the first
            // place (the reviewer is looking at the tree cursor, not a
            // scrolled right pane), and `handle_scroll_key` also no-ops in
            // that case, so this arm's blanket-reset behavior on Tree focus
            // is a harmless zero-to-zero write rather than a data loss.
            (
                Screen::Entry,
                _,
                InputKey::ScrollHalfPageDown
                | InputKey::ScrollHalfPageUp
                | InputKey::ScrollToTop
                | InputKey::ScrollToBottom,
            ) => {}
            // Unreachable while `handle_key` is entered directly (the popup
            // interception above returns before this match whenever
            // `jump_popup.is_some()`), kept only so the match stays
            // exhaustive against future refactors — same reasoning as the
            // `ToggleHelp` arm just above.
            (Screen::Entry, _, InputKey::PopupConfirm | InputKey::PopupCancel) => {}
        }

        if !preserve_scroll && !preserve_scroll_after_jump {
            self.right_pane_scroll = 0;
        }

        self
    }

    /// Handles one [`InputKey`] while the jump-target popup (ADR 0022) is
    /// open — mirrors the help overlay's own "takes over the whole key
    /// space" structure (`Self::handle_key`'s own doc comment): `Up`/`Down`
    /// move the popup's own selection cursor (clamped, not wrapping, same
    /// convention `Nav::handle`'s `CursorUp`/`CursorDown` already use),
    /// `PopupConfirm` jumps to the highlighted candidate and closes the
    /// popup, `PopupCancel` closes it without jumping, and every other key
    /// is swallowed as a no-op.
    fn handle_key_with_popup_open(mut self, key: InputKey) -> Self {
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
    /// Applies one of ADR 0026's four scroll [`InputKey`] variants against
    /// whichever pane is scrollable right now, given `viewport_height` — the
    /// last-drawn inner height of that pane, threaded in by `crate::run_app`
    /// (which knows it from [`crate::ui::draw`]'s return value) since `App`
    /// itself has no notion of the pane's layout.
    ///
    /// Split off from [`Self::handle_key`] rather than folded into it so
    /// the other ~20 [`InputKey`] variants — which don't need viewport
    /// height — don't pay the plumbing cost. `crate::run_app`'s two-step
    /// dispatch is: call [`Self::handle_key`] first for the blanket
    /// bookkeeping (`status`/`pending_prefix` reset, and — on the entry
    /// view — `preserve_scroll` bookkeeping), then call this method for
    /// the four scroll variants only. [`Self::handle_key`]'s own arms
    /// for these four variants are deliberate no-ops that document this
    /// split.
    ///
    /// Scoping:
    ///
    /// - On [`Screen::Source`], acts on `Screen::Source::scroll_top`.
    /// - On [`Screen::Entry`] + [`Focus::Right`], acts on
    ///   [`Self::right_pane_scroll`], the same field plain `j`/`k`
    ///   already updates while Right-focused.
    /// - On [`Screen::Entry`] + [`Focus::Tree`], a no-op — Tree-focused
    ///   motion belongs on the tree cursor, not on any pane's scroll.
    ///
    /// `usize::MAX` is used as the "scroll to bottom" sentinel
    /// ([`InputKey::ScrollToBottom`]'s doc comment): the clamp-at-draw
    /// step folds it down to `total_lines - viewport_height` cleanly,
    /// so no per-pane bottom sentinel is needed here.
    /// While the `?` help overlay is open, these four variants act on
    /// [`Self::help_scroll`] instead of whatever `self.screen`/`self.focus`
    /// would otherwise imply, checked before the screen/focus match below —
    /// same priority [`Self::handle_key`] already gives the overlay, and
    /// for the same reason: without this, `crate::run_app`'s unconditional
    /// second-step call to this method (ADR 0026's two-step dispatch;
    /// `Self::handle_key`'s own arms for these variants are deliberate
    /// no-ops) would fall through to the ordinary `Screen::Entry` +
    /// `Focus::Right` branch and silently scroll the right pane *behind*
    /// the overlay while it looked closed to the reviewer.
    pub fn handle_scroll_key(mut self, key: InputKey, viewport_height: usize) -> Self {
        let step = viewport_height / 2;
        if self.help_open {
            match key {
                InputKey::ScrollHalfPageDown => {
                    self.help_scroll = self.help_scroll.saturating_add(step);
                }
                InputKey::ScrollHalfPageUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(step);
                }
                InputKey::ScrollToTop => {
                    self.help_scroll = 0;
                }
                InputKey::ScrollToBottom => {
                    self.help_scroll = usize::MAX;
                }
                _ => {}
            }
            return self;
        }
        match (&self.screen, self.focus, key) {
            (
                Screen::Source {
                    symbol_id,
                    scroll_top,
                },
                _,
                InputKey::ScrollHalfPageDown,
            ) => {
                let next = scroll_top.saturating_add(step);
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: next,
                };
            }
            (
                Screen::Source {
                    symbol_id,
                    scroll_top,
                },
                _,
                InputKey::ScrollHalfPageUp,
            ) => {
                let next = scroll_top.saturating_sub(step);
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: next,
                };
            }
            (Screen::Source { symbol_id, .. }, _, InputKey::ScrollToTop) => {
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: 0,
                };
            }
            (Screen::Source { symbol_id, .. }, _, InputKey::ScrollToBottom) => {
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: usize::MAX,
                };
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollHalfPageDown) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_add(step);
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollHalfPageUp) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_sub(step);
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollToTop) => {
                self.right_pane_scroll = 0;
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollToBottom) => {
                self.right_pane_scroll = usize::MAX;
            }
            // Tree focus on the entry view, or any non-scroll key on the
            // source screen — deliberate no-op. `crate::run_app` only
            // calls this for the four scroll variants, so the non-scroll
            // case is defensive.
            _ => {}
        }
        self
    }
}
