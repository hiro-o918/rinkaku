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
use crate::nav::{Action, Nav};
use crate::order::{DirRank, OrderMode, rank_directories};
use crate::tree::{NodeKind, Tree, build_tree};
use rinkaku_core::render::Report;
use std::collections::HashMap;

mod input_key;
pub use input_key::InputKey;

#[cfg(test)]
#[path = "tests.rs"]
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
}

impl App {
    /// Builds the initial application state from `report`: the directory
    /// tree, its topological ranks, and a fresh [`Nav`] with everything
    /// expanded and the cursor on the first row (`Nav::new`'s own doc
    /// comment). Starts on [`Screen::Entry`] in [`OrderMode::Topological`]
    /// (ADR 0016 decision 4's default), ordered immediately so the first
    /// frame already reflects it rather than showing source order for one
    /// tick.
    pub fn new(report: &Report) -> Self {
        let mut tree = build_tree(report);
        let ranks = rank_directories(report);
        let order_mode = OrderMode::default();
        crate::order::order_tree(&mut tree, &ranks, order_mode);

        Self {
            tree,
            nav: Nav::new(),
            ranks,
            order_mode,
            screen: Screen::Entry,
            right_pane: RightPane::default(),
            blast_radius_return_pane: RightPane::default(),
            right_pane_scroll: 0,
            focus: Focus::default(),
            help_open: false,
            pending_prefix: None,
            jump_popup: None,
            jump_back: Vec::new(),
            jump_forward: Vec::new(),
            status: None,
            should_quit: false,
        }
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

    /// Which pane currently receives motion keys (ADR 0020) — see [`Focus`]'s
    /// own doc comment.
    pub fn focus(&self) -> Focus {
        self.focus
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
            NodeKind::Dir => None,
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
            NodeKind::Symbol(_) => BlastRadiusSelection::NotApplicable,
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

    /// Applies one [`InputKey`] and returns the next `App`. `report` is
    /// needed only for [`InputKey::Source`] (to confirm the row under the
    /// cursor is a present symbol before switching screens — the actual
    /// file read happens later, in `crate::run`, once `Screen::Source` is
    /// active) and is otherwise unused.
    ///
    /// The `?` help overlay (ADR 0020) is handled first and takes over the
    /// whole key space while open: `ToggleHelp` closes it and every other
    /// key is swallowed as a no-op (deliberately, including `Quit` — the
    /// overlay's whole point is a safe, low-stakes "let me check the keys"
    /// action that cannot be short-circuited by an accidental app exit; see
    /// `Self::help_open`'s own doc comment). This must run before the
    /// screen/focus dispatch below, not as another arm inside it, so no
    /// future `InputKey` variant can accidentally bypass the overlay by
    /// being handled in a screen-specific branch first.
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
            if key == InputKey::ToggleHelp {
                self.help_open = false;
            }
            return self;
        }
        if key == InputKey::ToggleHelp {
            self.help_open = true;
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
                    // drills in).
                    Some(NodeKind::Dir) => {
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
    pub fn handle_scroll_key(mut self, key: InputKey, viewport_height: usize) -> Self {
        let step = viewport_height / 2;
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
