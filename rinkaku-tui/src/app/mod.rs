//! Interactive application state (stage B, ADR 0015/0016): composes the
//! stage-A view-models (`crate::tree`, `crate::nav`, `crate::order`,
//! `crate::detail`) into one state machine driven by user key input.
//!
//! [`App::handle_key`] is a pure transition — no `ratatui`/`crossterm`
//! types in this module's public signatures, mirroring the discipline
//! `crate::nav`'s doc comment already establishes. The event loop
//! (`crate::run`) is the only place that translates a real
//! `crossterm::event::KeyEvent` into this module's [`InputKey`] and calls
//! into `ratatui` to draw.

use crate::detail::{
    DetailView, DirDetail, FileDetail, SymbolMention, build_detail, build_dir_detail,
    build_file_detail,
};
use crate::nav::Nav;
use crate::order::{DirRank, OrderMode, rank_directories};
use crate::review::ReviewState;
use crate::tree::{NodeKind, Tree, build_tree};
use rinkaku_core::render::Report;
use std::collections::HashMap;

mod handle_key;
mod input_key;
pub use input_key::InputKey;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

/// Which pane currently receives motion keys (ADR 0020): [`Focus::Tree`]
/// routes `j`/`k` to the tree cursor (today's behavior, unchanged), while
/// [`Focus::Right`] routes them to the right pane's scroll offset instead.
/// Independent of [`RightPane`] (which content is showing) and [`Screen`]
/// (entry vs. source) — a focus change never itself changes what content is
/// displayed, only which keys drive it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Tree,
    Right,
}

/// Which pane is currently on screen. The directory tree (`Entry`) is
/// always the spine; `Source` is a drill-down reached from a symbol row
/// and returns to `Entry` on `InputKey::Back` (ADR 0015: "the reviewer
/// never leaves the terminal to open an editor", reached on demand rather
/// than replacing the entry view permanently).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Entry,
    /// `symbol_id` is the symbol whose source is shown, kept as an id
    /// (not owned source text) so `App` stays cheap to clone/compare in
    /// tests — `crate::run`'s event loop resolves the actual file content
    /// via `crate::source` only when it redraws.
    ///
    /// `scroll_top` (ADR 0026) is the 0-based first-visible-line offset
    /// requested by the reviewer — an unclamped request the same shape
    /// [`App::right_pane_scroll`] uses. [`crate::ui::draw_source_screen`]
    /// clamps it against the file's actual line count and the pane's
    /// rendered height at draw time, keeping [`App`] free of any layout
    /// concern.
    ///
    /// Initialized by `crate::run_app` (when the `s` key transitions into
    /// this screen) to the same centered start [`crate::source::visible_window`]
    /// already computes, so the first frame still shows the symbol's
    /// definition centered in the viewport. Subsequent motion keys
    /// (`j`/`k`/`Ctrl-d`/`Ctrl-u`/`gg`/`G`, ADR 0026) update this field
    /// via [`App::handle_key`]/[`App::handle_scroll_key`] rather than
    /// re-centering per frame — auto-recentering while the reviewer is
    /// scrolling was the "wrong end of the design space" ADR 0026's
    /// Context calls out.
    ///
    /// `usize::MAX` is the sentinel for "scroll to bottom": the
    /// clamp-at-draw step folds it down to `total_lines - viewport_height`
    /// cleanly, so no separate variant is needed for that state (see
    /// ADR 0026's Alternatives).
    Source {
        symbol_id: String,
        scroll_top: usize,
    },
}

/// Which content the right-hand pane shows on [`Screen::Entry`] (TUI
/// iteration 2/ADR 0019, named "blast radius" per ADR 0023): the existing
/// signature/used-by/callers detail, the raw diff hunks touching the
/// selected row, or the dependency tree rooted at the selected directory/
/// file's path. Independent of [`Screen`] — it is a display mode for the
/// entry view's right pane, not a separate screen reached via drill-down
/// the way [`Screen::Source`] is.
///
/// [`RightPane::BlastRadius`] carries no path of its own — unlike a
/// hypothetical `BlastRadius(String)` variant, the rooted path is always
/// read fresh off the cursor's current row
/// (`App::selected_blast_radius_view`) each time the pane is drawn, the
/// same way [`RightPane::Detail`]/[`RightPane::Diff`] already derive their
/// content from the cursor rather than storing it. This is what makes
/// "follow the cursor while active" (ADR 0019) free: moving the cursor
/// while already in `BlastRadius` mode need not touch `RightPane` at all,
/// only re-run the lookup the next time `crate::ui` draws.
///
/// Defaults to [`Self::Diff`] (ADR 0020): "what changed" is what a
/// reviewer wants to see first, ahead of the aggregated used-by/callers
/// view `Detail` shows. `App::with_entry_pivot` (the `--entry --tui`
/// startup path) still overrides this default unconditionally by setting
/// `right_pane` to `BlastRadius` itself right after `App::new`, so this
/// default only matters for the ordinary (non-`--entry`) startup path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightPane {
    Detail,
    #[default]
    Diff,
    BlastRadius,
}

/// Whether the Diff pane renders unified (interleaved `-`/`+` lines) or
/// split (side-by-side old/new columns) content (ADR 0044). A per-`App`
/// mode, independent of the current row selection — toggling `v`/`V`
/// ([`InputKey::ToggleSplitView`]) keeps showing split (or unified) as the
/// cursor moves to a different row, the same way [`RightPane`] already
/// persists across cursor moves.
///
/// Defaults to `Split` (ADR 0044 amendment): dogfooding found split the
/// more useful opening state for the pane's usual case (a signature or
/// small block edit), and `MIN_SPLIT_VIEW_WIDTH`'s narrow-terminal
/// fallback already keeps a cramped pane on unified regardless of this
/// default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffViewMode {
    Unified,
    #[default]
    Split,
}

/// The right-hand pane's content for the row currently under the cursor
/// (TUI iteration 2), unifying what used to be [`App::selected_detail`]'s
/// symbol-only contract: a directory or file row now has its own detail
/// too (`crate::detail::build_dir_detail`/`build_file_detail`), rather than
/// falling through to the placeholder every non-symbol row used to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectedDetail {
    Symbol(DetailView),
    Dir(DirDetail),
    File(FileDetail),
}

/// What [`App::selected_diff_target`] resolved the cursor's row to — plain
/// data describing which file the diff pane should slice hunks from;
/// `crate::ui` combines this with the raw diff text (via
/// `crate::diff_view`) at draw time.
///
/// Per ADR 0027 this always resolves to a file-scoped target, even for
/// symbol rows — the "which symbol is focused" information is carried by
/// [`App::selected_diff_focus`] on a separate accessor and applied by
/// `crate::run_app` as an auto-scroll offset, not by branching the diff
/// pane's shape here. The old `DiffTarget::Symbol` variant is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTarget {
    File { path: String },
}

/// Which symbol (if any) the tree cursor is currently on for the diff
/// pane's benefit (ADR 0027 decision 2 + Consequences): `crate::run_app`
/// looks up this symbol's shaped section
/// (`crate::diff_shape::section_start_line_for_symbol`) and auto-scrolls
/// [`App::right_pane_scroll`] to that section's start whenever a new
/// selection triggers a diff-pane recompute. `None` on file/directory
/// rows and on removed symbol rows — those either have no symbol to
/// focus, or no line-range/graph identity to derive a section from.
///
/// `path` is redundant with [`DiffTarget::File`]'s own `path` (both come
/// from the same tree row), but is kept here so a caller with only a
/// `DiffFocus` in hand does not need to also thread a `DiffTarget`
/// through to know which file the focus belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFocus {
    pub path: String,
    pub symbol_id: String,
}

/// What [`App::selected_blast_radius_view`] resolved the cursor's row to
/// (ADR 0019, named "blast radius" per ADR 0023) — see that method's own
/// doc comment for the three-way split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlastRadiusSelection {
    NotApplicable,
    Empty { path: String },
    View(crate::blast_radius::BlastRadiusView),
}

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
struct JumplistEntry {
    symbol_id: String,
    right_pane_scroll: usize,
}

/// The jumplist's cap (ADR 0022 decision 4): oldest entries are dropped
/// once the back-stack would exceed this, since a reviewing session
/// realistically never needs more and an unbounded stack is an unnecessary
/// unbounded-growth risk for a long-running TUI session.
const JUMPLIST_CAP: usize = 100;

/// The whole interactive application's state: the stage-A view-models
/// composed together, plus which screen is active and a status-line
/// message for the caller to render. Rebuilt once per `Report` (in
/// [`App::new`]) and then evolved purely via [`App::handle_key`] — no
/// field here is re-derived from IO after construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    tree: Tree,
    nav: Nav,
    ranks: HashMap<String, DirRank>,
    order_mode: OrderMode,
    screen: Screen,
    right_pane: RightPane,
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
    blast_radius_return_pane: RightPane,
    /// Whether the Diff pane renders unified or split content (ADR 0044) —
    /// see [`DiffViewMode`]'s own doc comment.
    diff_view_mode: DiffViewMode,
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
    right_pane_scroll: usize,
    /// Which pane receives motion keys (ADR 0020) — see [`Focus`]'s own doc
    /// comment.
    focus: Focus,
    /// Whether the `?` help overlay (ADR 0020) is currently open. Kept as a
    /// flag rather than folded into [`Screen`]: the overlay is meant to sit
    /// *on top of* whatever screen/pane was already showing (so closing it
    /// returns exactly there), not replace it the way [`Screen::Source`]
    /// replaces the entry view — a `Screen` variant would have to carry the
    /// prior screen along just to restore it, which this flag avoids for
    /// free by construction: nothing else about `App`'s state changes while
    /// the overlay is open.
    help_open: bool,
    /// The `?` help overlay's own scroll offset (lines), unclamped in the
    /// same "requested, not authoritative" sense as [`Self::right_pane_scroll`]
    /// — `App` has no notion of the overlay's rendered height, so clamping
    /// is `crate::ui`'s job at draw time (`crate::ui::overlay::draw_help_overlay`,
    /// reusing [`crate::ui::scroll::render_scrollable_pane`]'s clamp).
    /// Reset to 0 whenever the overlay opens or closes ([`Self::handle_key`]'s
    /// `ToggleHelp` arms), so re-opening the overlay after scrolling always
    /// starts from the top rather than resuming a stale offset the reviewer
    /// has no way to see coming back.
    help_scroll: usize,
    /// A `g`-prefixed sequence's first key, awaiting its second (ADR 0022) —
    /// `None` outside that one-key window. Set by `g` and cleared by
    /// *every* subsequent key regardless of what it is (`crate::lib::
    /// translate_key` owns the actual resolution into `GotoDefinition`/
    /// `GotoReferences`/fall-through, this field only remembers that `g` was
    /// the previous key so `translate_key` has something to consult).
    pending_prefix: Option<PendingPrefix>,
    /// The jump-target popup's state while open (ADR 0022), `None`
    /// otherwise — see [`JumpPopup`]'s own doc comment.
    jump_popup: Option<JumpPopup>,
    /// The jumplist's back-stack (ADR 0022): locations to return to via
    /// `Ctrl-o`, most-recently-visited last. Capped at [`JUMPLIST_CAP`].
    jump_back: Vec<JumplistEntry>,
    /// The jumplist's forward-stack: locations to return to via `Ctrl-i`
    /// after at least one `Ctrl-o`. Cleared whenever a new jump
    /// (`GotoDefinition`/`GotoReferences`) is made from the middle of
    /// history, mirroring vim's own jumplist (a new jump abandons the
    /// forward history rather than preserving it).
    jump_forward: Vec<JumplistEntry>,
    /// A transient message for the status line (e.g. a source-read
    /// failure) — cleared on the next action that doesn't re-set it, so a
    /// stale error doesn't linger forever once the user has moved on.
    status: Option<String>,
    should_quit: bool,
    /// The review-notes feature's own state (ADR 0048) — `App` holds
    /// exactly this one field of it, per the ADR's Module boundary
    /// decision; every review-specific transition lives on [`ReviewState`]
    /// itself, not here.
    review: ReviewState,
    /// Whether sink A (posting a GitHub PR review) is on the export menu
    /// this session — mirrors whether `crate::session::TuiSession::run`
    /// was given a `PrContext`/submitter port, fixed for the session's
    /// lifetime (set once via [`Self::with_review_sink_a_available`], never
    /// by [`Self::handle_key`] itself). Kept on `App` rather than threaded
    /// as a `handle_key` parameter since `ReviewState::confirm_export`
    /// needs it and `App` is the only layer that both dispatches keys and
    /// is told this flag at startup.
    review_sink_a_available: bool,
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
            review_sink_a_available: false,
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

    /// The review-notes feature's own state (ADR 0048) — see
    /// [`crate::review::ReviewState`]'s own doc comment.
    pub fn review(&self) -> &ReviewState {
        &self.review
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
    /// ([`InputKey::NoteCompose`]'s own doc comment on why that key is
    /// special-cased before dispatch rather than routed through
    /// `handle_key`).
    pub fn with_review(mut self, review: ReviewState) -> Self {
        self.review = review;
        self
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// The detail-pane content for the row currently under the cursor
    /// (TUI iteration 2): a symbol's [`DetailView`], or a directory/file
    /// row's own [`DirDetail`]/[`FileDetail`] — `None` only when there are
    /// no rows at all, the cursor sits on a *removed* symbol (no detail to
    /// build, see `build_detail`'s doc comment), or `report`/`tree` no
    /// longer agree with each other (defensive — both should come from the
    /// same `App::new` call). `report` is threaded in per call rather than
    /// stored on `App` itself, since every `build_*` function here is
    /// already a cheap pure lookup and storing a whole `Report` on every
    /// `App` would duplicate data the caller (`crate::run`) already owns
    /// for the process's lifetime.
    pub fn selected_detail(&self, report: &Report) -> Option<SelectedDetail> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => {
                build_detail(report, &symbol_ref.id).map(SelectedDetail::Symbol)
            }
            NodeKind::Symbol(_) => None,
            NodeKind::Dir => {
                build_dir_detail(&self.tree, report, &row.node.path).map(SelectedDetail::Dir)
            }
            NodeKind::File => {
                build_file_detail(&self.tree, report, &row.node.path).map(SelectedDetail::File)
            }
            // A section's synthetic path never appears in `report.graph`,
            // so `build_dir_detail`'s cycle/fan-in lookups would find
            // nothing — falls back to the generic placeholder, same as a
            // removed symbol. Its children still get full detail once the
            // cursor moves onto them. A test group's synthetic path is the
            // same story.
            NodeKind::Section(_) | NodeKind::TestGroup { .. } => None,
        }
    }

    /// What the diff pane (TUI iteration 2, [`RightPane::Diff`]) should
    /// slice out of the raw diff text for the row currently under the
    /// cursor: a file-scoped [`DiffTarget::File`] on both file rows and
    /// symbol rows (ADR 0027 decision 1 — the diff pane always renders the
    /// whole file, and "which symbol is focused" is carried on
    /// [`Self::selected_diff_focus`] alongside), or `None` on a directory
    /// row (a directory spans multiple files with no single diff to show —
    /// showing "every hunk under this directory" was considered and
    /// deferred, since it would just be the concatenation of every file's
    /// own diff, better browsed file by file). `None` also when there are
    /// no rows at all.
    ///
    /// `_report` is unused now that resolution needs only the tree row's
    /// own path (previously the symbol variant needed the line range from
    /// `report.files[..].symbols` — ADR 0027 folded that lookup into
    /// `crate::diff_shape` instead). Kept in the signature so the
    /// symmetry with [`Self::selected_detail`]/[`Self::selected_diff_focus`]
    /// stays intact for call sites that already thread `report` through.
    pub fn selected_diff_target(&self, _report: &Report) -> Option<DiffTarget> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => Some(DiffTarget::File {
                path: row.node.path.clone(),
            }),
            NodeKind::Symbol(_) => None,
            NodeKind::File => Some(DiffTarget::File {
                path: row.node.path.clone(),
            }),
            // A section spans multiple files, same reasoning as `Dir`
            // above — no single diff to show (ADR 0035 Phase B). A test
            // group's synthetic path is likewise not a real file path.
            NodeKind::Dir | NodeKind::Section(_) | NodeKind::TestGroup { .. } => None,
        }
    }

    /// The label [`crate::ui::diff_pane`] shows on line 1 of its pinned
    /// header for the row currently under the cursor: a present symbol's
    /// own name (paired with the path on a symbol row), or a
    /// file/skipped-file row's path (rendered bare) — mirrors
    /// [`Self::selected_diff_target`]'s row-kind scoping (present symbol
    /// or file only) so the header never names a row the pane would not
    /// actually render a diff for.
    pub fn selected_diff_header_name(&self) -> Option<&str> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => Some(symbol_ref.name.as_str()),
            NodeKind::File => Some(row.node.path.as_str()),
            _ => None,
        }
    }

    /// Which symbol the tree cursor currently focuses for the diff pane's
    /// auto-scroll (ADR 0027 decision 2 + Consequences): [`DiffFocus`] on a
    /// present symbol row, `None` on file/directory rows, on removed symbol
    /// rows (no graph identity to look up), or when there are no rows at
    /// all. `report` is threaded through only defensively — the focus id
    /// itself lives on the tree row already, but a caller wiring the focus
    /// into the shaped diff content must still know whether the id exists
    /// in `report.files[..].symbols`; when it does not (a mismatch between
    /// tree and report, "should not happen" but not enforceable at compile
    /// time), returning `None` here matches
    /// [`crate::diff_shape::section_start_line_for_symbol`]'s own "no
    /// section found" behavior so the diff pane simply does not auto-scroll
    /// rather than jumping to a stale offset.
    pub fn selected_diff_focus(&self, report: &Report) -> Option<DiffFocus> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        let NodeKind::Symbol(symbol_ref) = &row.node.kind else {
            return None;
        };
        if symbol_ref.removed {
            return None;
        }
        let known = report
            .files
            .iter()
            .find(|file| file.path == row.node.path)
            .is_some_and(|file| file.symbols.iter().any(|s| s.id == symbol_ref.id));
        if !known {
            return None;
        }
        Some(DiffFocus {
            path: row.node.path.clone(),
            symbol_id: symbol_ref.id.clone(),
        })
    }

    /// What the blast-radius pane ([`RightPane::BlastRadius`], ADR 0019/0023)
    /// should show for the row currently under the cursor:
    /// [`BlastRadiusSelection::View`] when the cursor sits on a directory or
    /// file row and at least one symbol falls under that row's path,
    /// [`BlastRadiusSelection::Empty`] for a directory/file row whose path
    /// matches no symbol at all (still a valid selection, just nothing to
    /// draw a tree from), or [`BlastRadiusSelection::NotApplicable`] on a
    /// symbol row or when there are no rows at all — mirroring
    /// `selected_diff_target`'s three-way split between "not this kind of
    /// row", "this kind of row but nothing to show", and "here is the
    /// content", except the blast-radius pane additionally needs to render
    /// its own "no symbols under `<path>`" message rather than reuse a
    /// diff-pane-style generic placeholder, hence the extra variant instead
    /// of `Option`.
    ///
    /// Not cached on `App` itself (ADR 0019's "recompute on toggle or
    /// cursor move while active, not per frame" stance) — but this method
    /// still recomputes from scratch (cost O(V+E), see
    /// `crate::blast_radius::build_blast_radius_view`'s own doc comment) on *every* call,
    /// so satisfying that stance is the caller's responsibility: `crate::run`'s
    /// event loop calls this once per handled key (when the blast-radius
    /// pane is active and the selection could have changed), caches the
    /// result, and hands the cached [`BlastRadiusSelection`] into
    /// `crate::ui::draw` — which must not call this method itself, since
    /// `terminal.draw` runs on every ~100ms idle poll tick as well as on an
    /// actual key press.
    pub fn selected_blast_radius_view(&self, report: &Report) -> BlastRadiusSelection {
        let rows = self.nav.rows(&self.tree);
        let Some(row) = rows.get(self.nav.cursor()) else {
            return BlastRadiusSelection::NotApplicable;
        };
        match &row.node.kind {
            // A section's synthetic path is not a real file-tree prefix,
            // so `build_blast_radius_view` would find nothing and report
            // `Empty` — misleading for "not applicable to this row kind",
            // so it's grouped with `Symbol` instead. A test group's
            // synthetic path has the same problem.
            NodeKind::Symbol(_) | NodeKind::Section(_) | NodeKind::TestGroup { .. } => {
                BlastRadiusSelection::NotApplicable
            }
            NodeKind::Dir | NodeKind::File => {
                match crate::blast_radius::build_blast_radius_view(report, &row.node.path) {
                    Some(view) => BlastRadiusSelection::View(view),
                    None => BlastRadiusSelection::Empty {
                        path: row.node.path.clone(),
                    },
                }
            }
        }
    }

    /// The id of the *present* (non-removed) symbol under the cursor, or
    /// `None` when the cursor is not on a symbol row at all, sits on a
    /// removed symbol (no graph presence to jump from — same reasoning as
    /// `selected_diff_target`'s own removed-symbol handling), or there are
    /// no rows at all. Used by `crate::run_app` to resolve `gd`/`gr`
    /// candidates (`crate::detail::symbol_mentions`) before calling back
    /// into `App` — see [`InputKey::GotoDefinition`]'s own doc comment on
    /// why that resolution cannot happen inside `App::handle_key` itself.
    pub fn selected_symbol_id(&self) -> Option<&str> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => Some(symbol_ref.id.as_str()),
            _ => None,
        }
    }

    /// Jumps the cursor to `symbol_id` (ADR 0022): pushes the *current*
    /// location onto the jumplist's back-stack (capped at
    /// [`JUMPLIST_CAP`], oldest dropped) and clears the forward-stack (a new
    /// jump abandons any history the reviewer had already jumped back past
    /// — vim's own jumplist does the same), then moves the tree cursor via
    /// [`Nav::move_cursor_to_symbol`] (expanding collapsed ancestors) and
    /// resets the scroll offset to 0 so the jumped-to symbol's content
    /// starts from its top. Focus is deliberately left untouched (ADR
    /// 0022's own "keep reading" rationale).
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
        self
    }

    /// Moves the tree cursor to `symbol_id` (ADR 0030: manual diff-pane
    /// scrolling syncs the cursor back to the visible symbol), expanding
    /// collapsed ancestors on the way via [`Nav::move_cursor_to_symbol`] —
    /// same underlying move [`Self::jump_to_symbol`] performs, but
    /// deliberately *not* that method: this sync must not push a jumplist
    /// entry (a scroll session through several symbols would otherwise
    /// flood `Ctrl-o`/`Ctrl-i`'s history with moves the reviewer never
    /// asked to navigate through — ADR 0022's jumplist is for explicit
    /// `gd`/`gr` jumps only) and must not reset [`Self::right_pane_scroll`]
    /// (the scroll offset that just triggered this sync is exactly the
    /// value the caller wants preserved — resetting it here would make the
    /// sync fight its own trigger). A no-op (returning `self` unchanged,
    /// no status message — unlike `jump_to_symbol`'s, since a missing
    /// symbol here is an ordinary transient case, e.g. mid-recompute after
    /// a report reload, not a reviewer-facing navigation failure) when no
    /// row's symbol id matches `symbol_id`.
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
    fn push_jumplist_entry(&mut self, entry: JumplistEntry) {
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
}
