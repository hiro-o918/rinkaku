//! [`App`]'s own struct definition plus construction, plain accessors, and
//! direct-overwrite setters, split out of `app/mod.rs` (ADR 0028) —
//! everything else that touches `App` (key dispatch, cursor-row selection
//! queries, jumplist navigation) has its own sibling module; this one is
//! "what `App` is made of and how to build/read/overwrite it directly".

use crate::nav::{self, Nav};
use crate::order::{DirRank, OrderMode, rank_directories};
use crate::review::ReviewState;
use crate::search::SearchState;
use crate::tree::{Tree, build_tree};
use rinkaku_core::render::Report;
use std::collections::HashMap;

use super::{DiffViewMode, Focus, JumpPopup, PendingPrefix, RightPane, Screen};

/// The whole interactive application's state: the stage-A view-models
/// composed together, plus which screen is active and a status-line
/// message for the caller to render. Rebuilt once per `Report` (in
/// [`App::new`]) and then evolved purely via [`App::handle_key`] — no
/// field here is re-derived from IO after construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub(super) tree: Tree,
    pub(super) nav: Nav,
    pub(super) ranks: HashMap<String, DirRank>,
    pub(super) order_mode: OrderMode,
    pub(super) screen: Screen,
    pub(super) right_pane: RightPane,
    /// Which non-`BlastRadius` [`RightPane`] to return to when the user
    /// leaves [`RightPane::BlastRadius`] via an `R` re-press
    /// (`InputKey::ToggleBlastRadius`) — always [`RightPane::Detail`] or
    /// [`RightPane::Diff`], never `BlastRadius` itself, since it exists
    /// only to answer "what was showing right before the user opened the
    /// blast-radius pane". Updated the moment `right_pane` transitions
    /// *into* `BlastRadius` (capturing whatever it was showing at that
    /// instant), left untouched while already in `BlastRadius` (so moving
    /// the cursor or scrolling while active does not disturb it), and
    /// consulted only by `ToggleBlastRadius`'s own re-press branch —
    /// `ToggleDiff` pressed from `BlastRadius` is a distinct, unconditional
    /// "go to Diff" gesture (see that branch's own comment) and does not
    /// read this field at all.
    pub(super) blast_radius_return_pane: RightPane,
    /// Whether the Diff pane renders unified or split content (ADR 0044) —
    /// see [`DiffViewMode`]'s own doc comment.
    pub(super) diff_view_mode: DiffViewMode,
    /// The user's requested scroll offset (in lines) into the right-hand
    /// pane's content, as an unclamped "how far down would the user like to
    /// be" value rather than an authoritative display position: `App` has
    /// no notion of the pane's rendered height (that is a `ratatui::Rect`
    /// only `crate::ui` sees at draw time), so clamping this to
    /// `content_len.saturating_sub(pane_height)` is `crate::ui`'s
    /// responsibility (`ui::clamp_scroll`) — keeping this module free of
    /// any layout concern, matching the rest of `App`'s pure-state
    /// discipline. Reset to 0 by every key `handle_key` processes *except*
    /// `InputKey::Up`/`Down` while [`Focus::Right`] on [`Screen::Entry`]
    /// (ADR 0020 folded scrolling onto the same physical keys as cursor
    /// movement, gated by focus — `handle_key`'s own doc comment on why
    /// this is a blanket rule rather than an enumerated list of "actions
    /// that change the pane's content": the cursor can move *indirectly*,
    /// e.g. a collapse retargeting it onto a different row, which an
    /// enumerated list is prone to missing).
    pub(super) right_pane_scroll: usize,
    /// Which pane receives motion keys (ADR 0020) — see [`Focus`]'s own doc
    /// comment.
    pub(super) focus: Focus,
    /// Whether the `?` help overlay (ADR 0020) is currently open. Kept as a
    /// flag rather than folded into [`Screen`]: the overlay is meant to sit
    /// *on top of* whatever screen/pane was already showing (so closing it
    /// returns exactly there), not replace it the way [`Screen::Source`]
    /// replaces the entry view — a `Screen` variant would have to carry the
    /// prior screen along just to restore it, which this flag avoids for
    /// free by construction: nothing else about `App`'s state changes while
    /// the overlay is open.
    pub(super) help_open: bool,
    /// The `?` help overlay's own scroll offset (lines), unclamped in the
    /// same "requested, not authoritative" sense as [`Self::right_pane_scroll`]
    /// — `App` has no notion of the overlay's rendered height, so clamping
    /// is `crate::ui`'s job at draw time (`crate::ui::overlay::draw_help_overlay`,
    /// reusing [`crate::ui::scroll::render_scrollable_pane`]'s clamp).
    /// Reset to 0 whenever the overlay opens or closes ([`Self::handle_key`]'s
    /// `ToggleHelp` arms), so re-opening the overlay after scrolling always
    /// starts from the top rather than resuming a stale offset the reviewer
    /// has no way to see coming back.
    pub(super) help_scroll: usize,
    /// A `g`-prefixed sequence's first key, awaiting its second (ADR 0022) —
    /// `None` outside that one-key window. Set by `g` and cleared by
    /// *every* subsequent key regardless of what it is (`crate::lib::
    /// translate_key` owns the actual resolution into `GotoDefinition`/
    /// `GotoReferences`/fall-through, this field only remembers that `g` was
    /// the previous key so `translate_key` has something to consult).
    pub(super) pending_prefix: Option<PendingPrefix>,
    /// The jump-target popup's state while open (ADR 0022), `None`
    /// otherwise — see [`JumpPopup`]'s own doc comment.
    pub(super) jump_popup: Option<JumpPopup>,
    /// The jumplist's back-stack (ADR 0022): locations to return to via
    /// `Ctrl-o`, most-recently-visited last. Capped at
    /// [`super::jump::JUMPLIST_CAP`].
    pub(super) jump_back: Vec<super::jump::JumplistEntry>,
    /// The jumplist's forward-stack: locations to return to via `Ctrl-i`
    /// after at least one `Ctrl-o`. Cleared whenever a new jump
    /// (`GotoDefinition`/`GotoReferences`) is made from the middle of
    /// history, mirroring vim's own jumplist (a new jump abandons the
    /// forward history rather than preserving it).
    pub(super) jump_forward: Vec<super::jump::JumplistEntry>,
    /// A transient message for the status line (e.g. a source-read
    /// failure) — cleared on the next action that doesn't re-set it, so a
    /// stale error doesn't linger forever once the user has moved on.
    pub(super) status: Option<String>,
    pub(super) should_quit: bool,
    /// The review-annotations feature's own state (ADR 0048) — `App` holds
    /// exactly this one field of it, per the ADR's Module boundary
    /// decision; every review-specific transition lives on [`ReviewState`]
    /// itself, not here.
    pub(super) review: ReviewState,
    /// The Source-view search feature's own state (ADR 0057), following
    /// [`Self::review`]'s identical "one field, own module" precedent —
    /// every search-specific transition lives on [`SearchState`] itself.
    pub(super) search: SearchState,
    /// Whether sink A (posting a GitHub PR review) is on the export menu
    /// this session — mirrors whether `crate::session::TuiSession::run`
    /// was given a `PrContext`/submitter port, fixed for the session's
    /// lifetime (set once via [`Self::with_review_sink_a_available`], never
    /// by [`Self::handle_key`] itself). Kept on `App` rather than threaded
    /// as a `handle_key` parameter since `ReviewState::confirm_export`
    /// needs it and `App` is the only layer that both dispatches keys and
    /// is told this flag at startup.
    pub(super) review_sink_a_available: bool,
    /// The latest released version rinkaku's background update check
    /// found, if any (`main.rs`'s version-check thread, delivered via
    /// [`Self::notify_update_available`]) — `None` until that check
    /// completes or finds nothing newer. Drives the status line's update
    /// hint and whether `u` opens [`Self::update_prompt_open`] at all.
    pub(super) update_available: Option<String>,
    /// Whether the update confirmation popup is currently open — reachable
    /// via `u`, once [`Self::update_available`] is `Some`. Mirrors
    /// [`Self::jump_popup`]'s flag-not-`Screen` shape for the same reason:
    /// it sits on top of whatever was already showing and must not disturb
    /// that state.
    pub(super) update_prompt_open: bool,
    /// Whether the reviewer confirmed the update popup — `run_app`/
    /// `TuiSession::run` read this once [`Self::should_quit`] is set to
    /// decide whether to run `self-update` after the terminal is restored.
    pub(super) update_requested: bool,
}

impl App {
    /// Builds the initial application state from `report`: the directory
    /// tree, its topological ranks, and a fresh [`Nav`] with everything
    /// expanded except `TestGroup` rows (`Nav::new_collapsing_test_groups`'s
    /// own doc comment) and the cursor on the first row. Starts on
    /// [`Screen::Entry`] in [`OrderMode::Topological`] (ADR 0016 decision
    /// 4's default), ordered immediately so the first frame already
    /// reflects it rather than showing source order for one tick.
    pub fn new(report: &Report) -> Self {
        let mut tree = build_tree(report);
        let ranks = rank_directories(report);
        let order_mode = OrderMode::default();
        crate::order::order_tree(&mut tree, &ranks, order_mode);

        Self {
            nav: Nav::new_collapsing_test_groups(&tree),
            tree,
            ranks,
            order_mode,
            screen: Screen::Entry,
            right_pane: RightPane::default(),
            blast_radius_return_pane: RightPane::default(),
            diff_view_mode: DiffViewMode::default(),
            right_pane_scroll: 0,
            focus: Focus::default(),
            help_open: false,
            help_scroll: 0,
            pending_prefix: None,
            jump_popup: None,
            jump_back: Vec::new(),
            jump_forward: Vec::new(),
            status: None,
            should_quit: false,
            review: ReviewState::default(),
            search: SearchState::default(),
            review_sink_a_available: false,
            update_available: None,
            update_prompt_open: false,
            update_requested: false,
        }
    }

    /// Sets whether sink A (a GitHub PR review) is on the export menu for
    /// this session — `crate::session::TuiSession::run` calls this once,
    /// right after [`Self::new`], with whether it was given a `PrContext`/
    /// submitter port (ADR 0048).
    pub fn with_review_sink_a_available(mut self, available: bool) -> Self {
        self.review_sink_a_available = available;
        self
    }

    /// Applies `--entry <path>`'s TUI wiring on top of an already-built
    /// `App` (`crate::run`'s composition root calls this once, right after
    /// [`App::new`], only when `main.rs`'s `--entry` flag was passed):
    /// moves the cursor onto the tree row matching `path`
    /// (`Nav::move_cursor_to_path`) and switches straight to
    /// [`RightPane::BlastRadius`], so the TUI opens exactly where the CLI's own
    /// `--entry` would have rooted the Markdown/JSON tree, rather than
    /// requiring the reviewer to hunt for the row and press `R` themselves.
    ///
    /// When no visible row's path matches `path` exactly (wrong path, a
    /// typo, or a path that only exists nested under a collapsed ancestor —
    /// not possible from a fresh `App::new`, which starts fully expanded,
    /// but kept as a defensive case rather than panicking), the cursor and
    /// right pane are left at `App::new`'s own defaults and a status-line
    /// note is set instead, mirroring `main.rs`'s `entry_pivot_empty_note`
    /// for the non-TUI path — this is what keeps `--entry <path> --tui` from
    /// being a silent no-op (previously: the flag never touched `App` at
    /// all, since `apply_entry_pivot` only re-roots `report.graph`, which
    /// the tree/nav pane and Detail's fan-in do not read).
    pub fn with_entry_pivot(mut self, path: &str) -> Self {
        if self.nav.move_cursor_to_path(&self.tree, path) {
            self.right_pane = RightPane::BlastRadius;
            // Deliberately `RightPane::Detail`, not `RightPane::default()`
            // (ADR 0020 made the default `Diff`): this session never
            // actually showed a pane before opening the blast-radius pane
            // straight in at startup, so there is no real "what was
            // showing before" to restore — `Detail` is this method's own
            // independent choice of `R`-re-press destination, unaffected
            // by `RightPane`'s default changing.
            self.blast_radius_return_pane = RightPane::Detail;
        } else {
            self.status = Some(format!("note: no tree row matches {path}"));
        }
        self
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn nav(&self) -> &Nav {
        &self.nav
    }

    pub fn order_mode(&self) -> OrderMode {
        self.order_mode
    }

    /// Every directory's computed [`DirRank`], keyed by path — exposed so
    /// `crate::ui`/`crate::row_view` can show the cycle-warning marker on
    /// a directory row without recomputing `rank_directories` (which would
    /// also require re-threading a `Report` reference into rendering just
    /// for this) or duplicating the map onto every row.
    pub fn ranks(&self) -> &HashMap<String, DirRank> {
        &self.ranks
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    pub fn right_pane(&self) -> RightPane {
        self.right_pane
    }

    /// Whether the Diff pane renders unified or split content (ADR 0044).
    pub fn diff_view_mode(&self) -> DiffViewMode {
        self.diff_view_mode
    }

    /// Which pane currently receives motion keys (ADR 0020) — see [`Focus`]'s
    /// own doc comment.
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The `?` help overlay's requested scroll offset (lines) — see
    /// [`Self::help_scroll`]'s own doc comment on why this is unclamped.
    pub fn help_scroll(&self) -> usize {
        self.help_scroll
    }

    /// Whether the `?` help overlay (ADR 0020) is currently open.
    pub fn help_open(&self) -> bool {
        self.help_open
    }

    /// Whether a `g`-prefixed sequence (ADR 0022) is awaiting its second key
    /// — consulted by `crate::lib::translate_key` to decide whether the next
    /// key press should resolve `gd`/`gr` rather than its own ordinary
    /// meaning.
    pub fn pending_prefix(&self) -> Option<PendingPrefix> {
        self.pending_prefix
    }

    /// The jump-target popup's state (ADR 0022) while it is open, `None`
    /// otherwise.
    pub fn jump_popup(&self) -> Option<&JumpPopup> {
        self.jump_popup.as_ref()
    }

    /// The user's requested scroll offset into the right-hand pane — see
    /// the `right_pane_scroll` field's own doc comment on why this is an
    /// unclamped request rather than an authoritative display position.
    pub fn right_pane_scroll(&self) -> usize {
        self.right_pane_scroll
    }

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    /// The review-annotations feature's own state (ADR 0048) — see
    /// [`crate::review::ReviewState`]'s own doc comment.
    pub fn review(&self) -> &ReviewState {
        &self.review
    }

    /// The Source-view search feature's own state (ADR 0057) — see
    /// [`crate::search::SearchState`]'s own doc comment.
    pub fn search(&self) -> &SearchState {
        &self.search
    }

    /// Whether sink A (posting a GitHub PR review) is on the export menu
    /// this session — see [`Self::with_review_sink_a_available`]'s own doc
    /// comment. `crate::ui::review_overlay` reads this to keep the export
    /// menu's *rendered* entries in sync with the entry list
    /// [`crate::review::ReviewState::confirm_export`] resolves the cursor
    /// against, so the two never disagree about what selecting position 0
    /// means.
    pub fn review_sink_a_available(&self) -> bool {
        self.review_sink_a_available
    }

    /// Replaces `App`'s [`ReviewState`] wholesale — used by
    /// `crate::lib::run_app` for the one review transition that needs data
    /// (a [`crate::review::SelectionSnapshot`]) `App::handle_key` cannot
    /// derive itself: opening the compose overlay
    /// ([`InputKey::AnnotationCompose`]'s own doc comment on why that key is
    /// special-cased before dispatch rather than routed through
    /// `handle_key`).
    pub fn with_review(mut self, review: ReviewState) -> Self {
        self.review = review;
        self
    }

    /// Replaces `App`'s [`SearchState`] wholesale — used by
    /// `crate::event_loop::run_app` for the one search transition that
    /// needs data (the Source view's own source lines) `App::handle_key`
    /// cannot derive itself: confirming a query
    /// ([`InputKey::SearchConfirm`]'s own doc comment on why that key is
    /// special-cased before dispatch rather than routed through
    /// `handle_key`, mirroring [`Self::with_review`]'s identical precedent
    /// for [`InputKey::AnnotationCompose`]).
    pub fn with_search(mut self, search: SearchState) -> Self {
        self.search = search;
        self
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// The latest released version found by the background update check
    /// (`main.rs`'s version-check thread), if any — see the field's own
    /// doc comment.
    pub fn update_available(&self) -> Option<&str> {
        self.update_available.as_deref()
    }

    /// Whether the update confirmation popup is currently open.
    pub fn update_prompt_open(&self) -> bool {
        self.update_prompt_open
    }

    /// Whether the reviewer confirmed the update popup — see the field's
    /// own doc comment.
    pub fn update_requested(&self) -> bool {
        self.update_requested
    }

    /// Records that a newer released version is available, called once by
    /// `crate::event_loop::run_app`'s event loop when the background
    /// version-check thread's `mpsc::Receiver` yields a version string
    /// (`main.rs`'s composition root spawns that thread; this method is
    /// the only seam through which its result reaches `App`, keeping this
    /// module free of the thread/channel itself). The popup is never
    /// opened from here: ADR 0062 moved the confirmation ahead of
    /// analysis, onto the ordinary terminal, so reaching the TUI at all
    /// means the reviewer was not asked and `u` is the only way in.
    pub fn notify_update_available(&mut self, version: impl Into<String>) {
        self.update_available = Some(version.into());
    }

    /// Sets the status-line message directly — used by `crate::run` to
    /// surface a source-read failure (`ADR 0016`: file reads are
    /// adapter-side IO, so a failure there is reported back into this
    /// pure state rather than handled inside this module).
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    /// Overwrites the right-hand pane's scroll offset directly to `scroll`
    /// — used by `crate::run_app`'s `]c`/`[c` hunk-jump handling
    /// (`InputKey::NextHunk`/`PrevHunk`) to set an exact target line rather
    /// than the relative +/-1 [`Self::handle_key`] applies for plain `j`/`k`
    /// scrolling. Not itself an [`InputKey`] variant/`handle_key` branch,
    /// since the jump target depends on the diff pane's shaped content
    /// (`crate::diff_shape`), which `App` has no access to — `crate::run_app`
    /// computes the target and calls this setter once it has one (see that
    /// function's own comment on why the computation lives there).
    pub fn with_right_pane_scroll(mut self, scroll: usize) -> Self {
        self.right_pane_scroll = scroll;
        self
    }

    /// Overwrites the `?` help overlay's scroll offset directly to `scroll`
    /// — used by `crate::run_app` to fold the actually-clamped, actually-
    /// rendered offset back into `App` after every draw
    /// (`crate::ui::DrawOutcome`'s own doc comment on why this fold-back
    /// exists: without it, repeated scrolling past the overlay's own end
    /// would keep incrementing this unclamped request with no visible
    /// effect, the same overshoot [`Self::with_right_pane_scroll`] already
    /// guards against for the right pane).
    pub fn with_help_scroll(mut self, scroll: usize) -> Self {
        self.help_scroll = scroll;
        self
    }

    /// Overwrites [`Screen::Source`]'s `scroll_top` to `scroll_top` —
    /// used by `crate::run_app` right after the `s` key transitions to
    /// [`Screen::Source`], to back-fill the centered starting position
    /// [`crate::source::visible_window`] computes (see
    /// [`InputKey::Source`]'s handling in [`Self::handle_key`]: the
    /// transition itself sets `scroll_top = 0`, and this method
    /// overwrites it with the centered value once `run_app` has loaded
    /// the file and knows the layout). A no-op when the current screen
    /// is [`Screen::Entry`] — defensive: callers are expected to check
    /// [`Self::screen`] before invoking this, but `App` does not trust
    /// that blindly.
    pub fn with_source_scroll_top(mut self, scroll_top: usize) -> Self {
        if let Screen::Source { symbol_id, .. } = &self.screen {
            self.screen = Screen::Source {
                symbol_id: symbol_id.clone(),
                scroll_top,
            };
        }
        self
    }

    /// Jumps the tree cursor directly to visible-row `index` (ADR 0057
    /// amendment: tree search) — `crate::event_loop::run_app` calls this
    /// once a confirmed search's current match is known, the same
    /// "`App` has no notion of X, caller computes it and folds it back"
    /// split [`Self::with_source_scroll_top`]/[`Self::with_right_pane_scroll`]
    /// already use for their own screen-specific jump targets. Delegates to
    /// [`nav::Action::CursorTo`] rather than writing `self.nav` directly, so
    /// the same out-of-bounds clamp every other cursor motion gets applies
    /// here too.
    pub fn with_nav_cursor(mut self, index: usize) -> Self {
        self.nav = self.nav.handle(nav::Action::CursorTo(index), &self.tree);
        self
    }
}
