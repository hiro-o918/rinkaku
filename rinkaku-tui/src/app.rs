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
    DetailView, DirDetail, FileDetail, build_detail, build_dir_detail, build_file_detail,
};
use crate::nav::{Action, Nav};
use crate::order::{DirRank, OrderMode, rank_directories};
use crate::tree::{NodeKind, Tree, build_tree};
use rinkaku_core::render::Report;
use std::collections::HashMap;

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
    /// Space, or Enter on a directory row: expand/collapse a directory row
    /// (`App::handle_key`'s doc comment) — never changes focus. Kept as a
    /// distinct variant from [`Self::Open`] (ADR 0020) because Space must
    /// never move focus even on a file/symbol row, only Enter does.
    Select,
    /// Enter on a file/symbol row: opens the source view on a symbol row
    /// (unchanged from before ADR 0020) and additionally moves focus to
    /// [`Focus::Right`] (ADR 0020's "drilling into a row is also a focus
    /// change") — a no-op on a directory row (`App::handle_key`'s doc
    /// comment; a directory row's Enter is [`Self::Select`]/`crate::run`'s
    /// `translate_key`, matching on `KeyCode::Enter`, always emits `Open`
    /// and lets `handle_key` decide what that means per row kind).
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
    /// `p`/`P`: toggle the right-hand pane between [`RightPane::Pivot`] and
    /// whichever mode was active before ([`RightPane::Detail`] or
    /// [`RightPane::Diff`]) — ADR 0019's entry-path pivot. Pressing `p`
    /// again while already in `Pivot` mode returns to the prior mode (stored
    /// in `App`'s `pivot_return_pane` field the moment `Pivot` was entered),
    /// mirroring `d`'s own toggle rather than a one-way "enter pivot mode"
    /// action, since the ADR describes `p` as a per-row toggle. Global
    /// regardless of [`Focus`] (ADR 0020).
    TogglePivot,
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
    /// `q` or Ctrl-C on the entry view: exit the application.
    Quit,
    /// `?`: opens the help overlay (ADR 0020). While the overlay is open,
    /// `?` instead closes it — `crate::run`'s `translate_key` maps the same
    /// physical key to this one variant either way, and `App::handle_key`
    /// treats it as a toggle.
    ToggleHelp,
}

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
    Source {
        symbol_id: String,
    },
}

/// Which content the right-hand pane shows on [`Screen::Entry`] (TUI
/// iteration 2/ADR 0019): the existing signature/used-by/callers detail,
/// the raw diff hunks touching the selected row, or the pivot tree rooted
/// at the selected directory/file's path. Independent of [`Screen`] — it is
/// a display mode for the entry view's right pane, not a separate screen
/// reached via drill-down the way [`Screen::Source`] is.
///
/// [`RightPane::Pivot`] carries no path of its own — unlike a hypothetical
/// `Pivot(String)` variant, the pivoted path is always read fresh off the
/// cursor's current row (`App::selected_pivot_view`) each time the pane is
/// drawn, the same way [`RightPane::Detail`]/[`RightPane::Diff`] already
/// derive their content from the cursor rather than storing it. This is
/// what makes "follow the cursor while pivoted" (ADR 0019) free: moving the
/// cursor while already in `Pivot` mode need not touch `RightPane` at all,
/// only re-run the lookup the next time `crate::ui` draws.
///
/// Defaults to [`Self::Diff`] (ADR 0020): "what changed" is what a
/// reviewer wants to see first, ahead of the aggregated used-by/callers
/// view `Detail` shows. `App::with_entry_pivot` (the `--entry --tui`
/// startup path) still overrides this default unconditionally by setting
/// `right_pane` to `Pivot` itself right after `App::new`, so this default
/// only matters for the ordinary (non-`--entry`) startup path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightPane {
    Detail,
    #[default]
    Diff,
    Pivot,
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
/// data describing which file (and, for a symbol, which 1-based inclusive
/// line range) the diff pane should slice hunks from; `crate::ui` combines
/// this with the raw diff text (via `crate::diff_view`) at draw time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTarget {
    Symbol {
        path: String,
        range_start: usize,
        range_end: usize,
    },
    File {
        path: String,
    },
}

/// What [`App::selected_pivot_view`] resolved the cursor's row to (ADR
/// 0019) — see that method's own doc comment for the three-way split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PivotSelection {
    NotApplicable,
    Empty { path: String },
    View(crate::pivot::PivotView),
}

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
    /// Which non-`Pivot` [`RightPane`] to return to when the user leaves
    /// [`RightPane::Pivot`] via a `p` re-press (`InputKey::TogglePivot`) —
    /// always [`RightPane::Detail`] or [`RightPane::Diff`], never `Pivot`
    /// itself, since it exists only to answer "what was showing right
    /// before the user pivoted". Updated the moment `right_pane` transitions
    /// *into* `Pivot` (capturing whatever it was showing at that instant),
    /// left untouched while already in `Pivot` (so moving the cursor or
    /// scrolling while pivoted does not disturb it), and consulted only by
    /// `TogglePivot`'s own re-press branch — `ToggleDiff` pressed from
    /// `Pivot` is a distinct, unconditional "go to Diff" gesture (see that
    /// branch's own comment) and does not read this field at all.
    pivot_return_pane: RightPane,
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
            pivot_return_pane: RightPane::default(),
            right_pane_scroll: 0,
            focus: Focus::default(),
            help_open: false,
            status: None,
            should_quit: false,
        }
    }

    /// Applies `--entry <path>`'s TUI wiring on top of an already-built
    /// `App` (`crate::run`'s composition root calls this once, right after
    /// [`App::new`], only when `main.rs`'s `--entry` flag was passed):
    /// moves the cursor onto the tree row matching `path`
    /// (`Nav::move_cursor_to_path`) and switches straight to
    /// [`RightPane::Pivot`], so the TUI opens exactly where the CLI's own
    /// `--entry` would have rooted the Markdown/JSON tree, rather than
    /// requiring the reviewer to hunt for the row and press `p` themselves.
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
            self.right_pane = RightPane::Pivot;
            // Deliberately `RightPane::Detail`, not `RightPane::default()`
            // (ADR 0020 made the default `Diff`): this session never
            // actually showed a pane before pivoting straight in at
            // startup, so there is no real "what was showing before" to
            // restore — `Detail` is this method's own independent choice
            // of `p`-re-press destination, unaffected by `RightPane`'s
            // default changing.
            self.pivot_return_pane = RightPane::Detail;
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
    /// cursor: a symbol row scopes to just its own line range (looked up
    /// from `report`, since `crate::tree::SymbolRef` itself carries no line
    /// range — only `id`/`name`/`kind`/`classification`/`removed`), a file
    /// row to the whole file, and a directory row has nothing diff-specific
    /// to show (a directory spans multiple files with no single line range
    /// to highlight — showing "every hunk under this directory" was
    /// considered and deferred, since it would just be the concatenation
    /// of every file's own diff, better browsed file by file). `None` when
    /// there are no rows at all, the cursor sits on a removed symbol (no
    /// line range to scope to, same as `selected_detail`'s handling), or
    /// (defensively) `report` no longer contains the selected symbol.
    pub fn selected_diff_target(&self, report: &Report) -> Option<DiffTarget> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => {
                let range = report
                    .files
                    .iter()
                    .find(|file| file.path == row.node.path)
                    .and_then(|file| file.symbols.iter().find(|s| s.id == symbol_ref.id))
                    .map(|s| s.range)?;
                Some(DiffTarget::Symbol {
                    path: row.node.path.clone(),
                    range_start: range.start,
                    range_end: range.end,
                })
            }
            NodeKind::Symbol(_) => None,
            NodeKind::File => Some(DiffTarget::File {
                path: row.node.path.clone(),
            }),
            NodeKind::Dir => None,
        }
    }

    /// What the pivot pane ([`RightPane::Pivot`], ADR 0019) should show for
    /// the row currently under the cursor: [`PivotSelection::View`] when the
    /// cursor sits on a directory or file row and at least one symbol falls
    /// under that row's path, [`PivotSelection::Empty`] for a directory/file
    /// row whose path matches no symbol at all (still a valid selection,
    /// just nothing to draw a tree from), or [`PivotSelection::NotApplicable`]
    /// on a symbol row or when there are no rows at all — mirroring
    /// `selected_diff_target`'s three-way split between "not this kind of
    /// row", "this kind of row but nothing to show", and "here is the
    /// content", except pivot additionally needs to render its own "no
    /// symbols under `<path>`" message rather than reuse a diff-pane-style
    /// generic placeholder, hence the extra variant instead of `Option`.
    ///
    /// Not cached on `App` itself (ADR 0019's "recompute on pivot toggle or
    /// cursor move while pivoted, not per frame" stance) — but this method
    /// still recomputes from scratch (cost O(V+E), see
    /// `crate::pivot::build_pivot_view`'s own doc comment) on *every* call,
    /// so satisfying that stance is the caller's responsibility: `crate::run`'s
    /// event loop calls this once per handled key (when the pivot pane is
    /// active and the selection could have changed), caches the result, and
    /// hands the cached [`PivotSelection`] into `crate::ui::draw` — which
    /// must not call this method itself, since `terminal.draw` runs on every
    /// ~100ms idle poll tick as well as on an actual key press.
    pub fn selected_pivot_view(&self, report: &Report) -> PivotSelection {
        let rows = self.nav.rows(&self.tree);
        let Some(row) = rows.get(self.nav.cursor()) else {
            return PivotSelection::NotApplicable;
        };
        match &row.node.kind {
            NodeKind::Symbol(_) => PivotSelection::NotApplicable,
            NodeKind::Dir | NodeKind::File => {
                match crate::pivot::build_pivot_view(report, &row.node.path) {
                    Some(view) => PivotSelection::View(view),
                    None => PivotSelection::Empty {
                        path: row.node.path.clone(),
                    },
                }
            }
        }
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
    pub fn handle_key(mut self, key: InputKey) -> Self {
        self.status = None;

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

        let preserve_scroll = matches!(
            (&self.screen, self.focus, key),
            (Screen::Entry, Focus::Right, InputKey::Up)
                | (Screen::Entry, Focus::Right, InputKey::Down)
        );

        match (&self.screen, self.focus, key) {
            (Screen::Source { .. }, _, InputKey::Back) => {
                self.screen = Screen::Entry;
            }
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
            (Screen::Entry, _, InputKey::Select) => {
                self.nav = self.nav.handle(Action::ToggleExpand, &self.tree);
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
                    Some(NodeKind::File) => {
                        self.focus = Focus::Right;
                    }
                    Some(NodeKind::Symbol(symbol_ref)) if !symbol_ref.removed => {
                        self.focus = Focus::Right;
                        self.screen = Screen::Source {
                            symbol_id: symbol_ref.id.clone(),
                        };
                    }
                    // A removed symbol has no source to open (mirrors
                    // `InputKey::Source`'s own `!symbol_ref.removed` guard
                    // below) and no row at all is simply a no-op.
                    Some(NodeKind::Symbol(_)) | None => {}
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
                    self.screen = Screen::Source {
                        symbol_id: symbol_ref.id.clone(),
                    };
                }
            }
            (Screen::Entry, _, InputKey::ToggleDiff) => {
                self.right_pane = match self.right_pane {
                    RightPane::Diff => RightPane::Detail,
                    // From Detail or Pivot, `d` always lands on Diff — a
                    // deliberate, unconditional "go to Diff" gesture rather
                    // than consulting `pivot_return_pane`. Only `p`'s own
                    // re-press (`TogglePivot` below) restores the
                    // pre-pivot pane; `d` pressed while pivoted is its own
                    // independent choice of destination, matching this
                    // arm's existing "from Detail, `d` always lands on
                    // Diff" behavior rather than growing a second "restore
                    // the previous pane" rule that would only apply to
                    // some keys and not others.
                    RightPane::Detail | RightPane::Pivot => RightPane::Diff,
                };
            }
            (Screen::Entry, _, InputKey::TogglePivot) => {
                self.right_pane = match self.right_pane {
                    // Restore whichever pane was showing right before this
                    // pivot session started, rather than unconditionally
                    // Detail — `pivot_return_pane` was captured below the
                    // moment `Pivot` was entered, so e.g. `d` -> `p` -> `p`
                    // returns to Diff, not Detail.
                    RightPane::Pivot => self.pivot_return_pane,
                    RightPane::Detail | RightPane::Diff => {
                        self.pivot_return_pane = self.right_pane;
                        RightPane::Pivot
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
            // wrapped to the pane's width), so `App` only remembers *that*
            // the key was pressed via a status-line no-op here today; the
            // actual scroll-offset jump is wired in `crate::ui`/`crate::run`
            // once the shaped diff content (this ADR's diff-scoping commit)
            // exists to jump against. Scoped to `Focus::Right` + a
            // `RightPane::Diff` check the same way scrolling itself is
            // scoped to focus, so pressing `]c` while Tree-focused or while
            // viewing Detail/Pivot is a no-op rather than silently doing
            // nothing that *looks* like it should have worked.
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
        }

        if !preserve_scroll {
            self.right_pane_scroll = 0;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail::FileSymbolSummary;
    use pretty_assertions::assert_eq;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;
    use rinkaku_core::render::FileReport;

    fn symbol(id: &str, name: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    fn empty_report() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        }
    }

    fn report_with_one_symbol() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo")],
            }],
            ..empty_report()
        }
    }

    #[test]
    fn should_start_on_entry_screen_with_topological_order_and_no_status() {
        let report = report_with_one_symbol();

        let app = App::new(&report);

        assert_eq!(Screen::Entry, *app.screen());
        assert_eq!(OrderMode::Topological, app.order_mode());
        assert_eq!(None, app.status());
        assert_eq!(false, app.should_quit());
    }

    #[test]
    fn should_set_should_quit_when_quit_is_pressed_on_entry_screen() {
        let report = empty_report();
        let app = App::new(&report);

        let app = app.handle_key(InputKey::Quit);

        assert_eq!(true, app.should_quit());
    }

    #[test]
    fn should_move_cursor_down_when_down_is_pressed() {
        // lib.rs has one file row and one symbol row; Down should move off
        // the initial cursor position (0) onto the symbol row (1).
        let report = report_with_one_symbol();
        let app = App::new(&report);

        let app = app.handle_key(InputKey::Down);

        assert_eq!(1, app.nav().cursor());
    }

    #[test]
    fn should_toggle_order_mode_between_topological_and_alpha_numeric() {
        let report = empty_report();
        let app = App::new(&report);
        assert_eq!(OrderMode::Topological, app.order_mode());

        let app = app.handle_key(InputKey::ToggleOrder);
        assert_eq!(OrderMode::AlphaNumeric, app.order_mode());

        let app = app.handle_key(InputKey::ToggleOrder);
        assert_eq!(OrderMode::Topological, app.order_mode());
    }

    #[test]
    fn should_open_source_screen_when_source_key_is_pressed_on_a_symbol_row() {
        let report = report_with_one_symbol();
        // Row 0 is the "lib.rs" file, row 1 is the "foo" symbol.
        let app = App::new(&report).handle_key(InputKey::Down);

        let app = app.handle_key(InputKey::Source);

        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );
    }

    #[test]
    fn should_stay_on_entry_screen_when_source_key_is_pressed_on_a_file_row() {
        let report = report_with_one_symbol();
        // Row 0 is the "lib.rs" file row itself, not a symbol.
        let app = App::new(&report);

        let app = app.handle_key(InputKey::Source);

        assert_eq!(Screen::Entry, *app.screen());
    }

    #[test]
    fn should_return_to_entry_screen_when_back_is_pressed_on_source_screen() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);
        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );

        let app = app.handle_key(InputKey::Back);

        assert_eq!(Screen::Entry, *app.screen());
    }

    #[test]
    fn should_ignore_navigation_keys_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);
        let cursor_before = app.nav().cursor();

        let app = app.handle_key(InputKey::Down);

        assert_eq!(cursor_before, app.nav().cursor());
        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );
    }

    #[test]
    fn should_clear_status_message_on_the_next_handled_key() {
        let report = empty_report();
        let mut app = App::new(&report);
        app.set_status("a source read failed");
        assert_eq!(Some("a source read failed"), app.status());

        let app = app.handle_key(InputKey::Down);

        assert_eq!(None, app.status());
    }

    #[test]
    fn should_return_file_detail_when_cursor_is_on_a_file_row() {
        // Row 0 is the "lib.rs" file itself, not a symbol (TUI iteration
        // 2: a file row now gets its own detail instead of `None`).
        let report = report_with_one_symbol();
        let app = App::new(&report);

        let actual = app.selected_detail(&report);

        let expected = SelectedDetail::File(FileDetail {
            path: "lib.rs".to_string(),
            symbols: vec![FileSymbolSummary {
                name: "foo".to_string(),
                kind: SymbolKind::Function,
                classification: None,
                removed: false,
                fan_in: 0,
            }],
            skip_reason: None,
            test_symbol_count: None,
        });
        assert_eq!(Some(expected), actual);
    }

    #[test]
    fn should_return_detail_view_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Down);

        let actual = app.selected_detail(&report);

        match actual.expect("detail for selected symbol") {
            SelectedDetail::Symbol(detail) => assert_eq!("foo", detail.name),
            other => panic!("expected SelectedDetail::Symbol, got {other:?}"),
        }
    }

    #[test]
    fn should_return_dir_detail_when_cursor_is_on_a_directory_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo")],
            }],
            ..empty_report()
        };
        let app = App::new(&report);

        let actual = app.selected_detail(&report);

        let expected = SelectedDetail::Dir(DirDetail {
            path: "src".to_string(),
            badges: crate::tree::Badges {
                changed_symbols: 1,
                contract_changes: 0,
                fan_in: 0,
            },
            top_fan_in: vec![],
            cycle_partners: vec![],
            cycle_edges: vec![],
        });
        assert_eq!(Some(expected), actual);
    }

    #[test]
    fn should_default_right_pane_to_diff() {
        // ADR 0020: "what changed" is what a reviewer wants first, ahead of
        // the aggregated used-by/callers view Detail shows.
        let report = empty_report();
        let app = App::new(&report);

        assert_eq!(RightPane::Diff, app.right_pane());
    }

    #[test]
    fn should_toggle_right_pane_between_diff_and_detail() {
        let report = empty_report();
        let app = App::new(&report);
        assert_eq!(RightPane::Diff, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);
        assert_eq!(RightPane::Detail, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);
        assert_eq!(RightPane::Diff, app.right_pane());
    }

    #[test]
    fn should_toggle_right_pane_between_diff_and_pivot() {
        let report = empty_report();
        let app = App::new(&report);
        assert_eq!(RightPane::Diff, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Pivot, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Diff, app.right_pane());
    }

    #[test]
    fn should_switch_from_pivot_to_diff_when_toggle_diff_is_pressed() {
        // ADR 0019: "p" re-press or "d" both leave pivot mode — "d" always
        // lands on Diff regardless of `pivot_return_pane` (a deliberate,
        // unconditional gesture — see `handle_key`'s `ToggleDiff` arm). Uses
        // Detail (not the default Diff) as the pane pivoted from, so this
        // test still shows something once "d" is pressed even though the
        // destination is unconditional either way.
        let report = empty_report();
        let app = App::new(&report)
            .handle_key(InputKey::ToggleDiff) // Diff -> Detail
            .handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Pivot, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);

        assert_eq!(RightPane::Diff, app.right_pane());
    }

    #[test]
    fn should_switch_from_detail_to_pivot_when_toggle_pivot_is_pressed() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleDiff); // Diff -> Detail
        assert_eq!(RightPane::Detail, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);

        assert_eq!(RightPane::Pivot, app.right_pane());
    }

    #[test]
    fn should_return_to_diff_when_pivot_is_toggled_off_after_entering_from_the_default_diff_pane() {
        // Pivoting straight from `App::new`'s own default (Diff, ADR 0020)
        // must restore Diff specifically on `p`'s re-press, pinning that
        // `pivot_return_pane` is actually captured on entry rather than
        // this behavior being a coincidence of `RightPane::default()`.
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Pivot, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);

        assert_eq!(RightPane::Diff, app.right_pane());
    }

    #[test]
    fn should_return_to_detail_when_pivot_is_toggled_off_after_entering_from_detail() {
        // Companion to the Diff-return-pane test above: pivoting from
        // Detail (reached via `d`, not the default) must still restore
        // Detail specifically, not "whatever the default happens to be".
        let report = empty_report();
        let app = App::new(&report)
            .handle_key(InputKey::ToggleDiff) // Diff -> Detail
            .handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Pivot, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);

        assert_eq!(RightPane::Detail, app.right_pane());
    }

    #[test]
    fn should_ignore_toggle_pivot_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);
        assert_eq!(RightPane::Diff, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);

        assert_eq!(RightPane::Diff, app.right_pane());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_toggling_pivot_pane() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Open)
            .handle_key(InputKey::Down);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::TogglePivot);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_return_not_applicable_pivot_selection_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Down);

        let actual = app.selected_pivot_view(&report);

        assert_eq!(PivotSelection::NotApplicable, actual);
    }

    #[test]
    fn should_return_pivot_view_when_cursor_is_on_a_directory_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo")],
            }],
            graph: rinkaku_core::graph::SymbolGraph {
                nodes: vec![rinkaku_core::graph::Node {
                    id: "src/lib.rs::foo".to_string(),
                    path: "src/lib.rs".to_string(),
                    name: "foo".to_string(),
                }],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            ..empty_report()
        };
        // Row 0 is the "src" directory itself (a single-child directory
        // collapsed with "src/lib.rs" would still leave "src" as the
        // top-level row — see `crate::tree::build_tree`'s collapsing rule;
        // this fixture's one file under one directory does not collapse
        // further since "src/lib.rs" is a file, not a subdirectory).
        let app = App::new(&report);

        let actual = app.selected_pivot_view(&report);

        match actual {
            PivotSelection::View(view) => assert_eq!("src".to_string(), view.path),
            other => panic!("expected PivotSelection::View, got {other:?}"),
        }
    }

    #[test]
    fn should_return_not_applicable_pivot_selection_when_there_are_no_rows_at_all() {
        // The cursor has no row to sit on when the tree itself is empty —
        // distinct from `should_return_empty_pivot_selection_when_file_row_path_matches_no_graph_node`
        // below, which pins the actual `PivotSelection::Empty` trigger
        // (a real File row whose path matches no graph node).
        let report = empty_report();
        let app = App::new(&report);

        let actual = app.selected_pivot_view(&report);

        assert_eq!(PivotSelection::NotApplicable, actual);
    }

    #[test]
    fn should_return_empty_pivot_selection_when_file_row_path_matches_no_graph_node() {
        // The real-world trigger for `PivotSelection::Empty` (not the
        // previous version of this test, which used an empty report and so
        // only ever exercised `NotApplicable` — the cursor had no row at
        // all): a `FileReport` with an empty `symbols` list (e.g. a file
        // whose only changes are comments, or a pure rename) still produces
        // a `File` tree row (`crate::tree::build_tree`'s own doc comment:
        // "a pure rename, still shown as a `File` node with zero badges"),
        // but contributes no node to `report.graph` at all — `graph` here
        // is deliberately left at `empty_report`'s empty default, mirroring
        // that mismatch. `App::new` starts fully expanded with the cursor
        // on the tree's first (and only) row, this file itself.
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![],
            }],
            ..empty_report()
        };
        let app = App::new(&report);

        let actual = app.selected_pivot_view(&report);

        assert_eq!(
            PivotSelection::Empty {
                path: "lib.rs".to_string()
            },
            actual
        );
    }

    /// Same shape as `report_with_two_directories`, but with a populated
    /// `graph` (that fixture leaves `graph` empty since none of its own
    /// nav-focused tests need one) — required for `selected_pivot_view` to
    /// return `PivotSelection::View` rather than `Empty` for either
    /// directory.
    fn report_with_two_directories_and_graph() -> Report {
        let report = report_with_two_directories();
        let graph = rinkaku_core::graph::build_graph(&report.files);
        Report { graph, ..report }
    }

    #[test]
    fn should_follow_cursor_when_moving_between_directory_rows_while_pivoted() {
        let report = report_with_two_directories_and_graph();
        let app = App::new(&report).handle_key(InputKey::TogglePivot);

        let first = match app.selected_pivot_view(&report) {
            PivotSelection::View(view) => view.path,
            other => panic!("expected PivotSelection::View, got {other:?}"),
        };
        assert_eq!("a".to_string(), first);

        // Row 0 is "a", row 3 is "b" (per report_with_two_directories's own
        // doc comment on expanded row order) — three Down presses land the
        // cursor on "b".
        let app = app
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Down);
        let second = match app.selected_pivot_view(&report) {
            PivotSelection::View(view) => view.path,
            other => panic!("expected PivotSelection::View, got {other:?}"),
        };
        assert_eq!("b".to_string(), second);
    }

    #[test]
    fn should_move_cursor_and_open_pivot_pane_when_entry_pivot_path_matches_a_row() {
        let report = report_with_two_directories_and_graph();
        let app = App::new(&report);
        // ADR 0020 made Diff the default right pane; this pins that
        // `with_entry_pivot` still unconditionally overrides it to Pivot
        // regardless, since it sets `right_pane` directly after `App::new`
        // rather than consulting `RightPane::default()`.
        assert_eq!(RightPane::Diff, app.right_pane());

        let app = app.with_entry_pivot("b");

        // Row 3 is "b" (per `report_with_two_directories`'s own doc comment
        // on expanded row order).
        assert_eq!(3, app.nav().cursor());
        assert_eq!(RightPane::Pivot, app.right_pane());
        assert_eq!(None, app.status());
        let selected = match app.selected_pivot_view(&report) {
            PivotSelection::View(view) => view.path,
            other => panic!("expected PivotSelection::View, got {other:?}"),
        };
        assert_eq!("b".to_string(), selected);
    }

    #[test]
    fn should_set_status_note_and_leave_defaults_when_entry_pivot_path_matches_no_row() {
        let report = report_with_two_directories_and_graph();
        let app = App::new(&report);

        let app = app.with_entry_pivot("no/such/path");

        assert_eq!(0, app.nav().cursor());
        assert_eq!(RightPane::Diff, app.right_pane());
        assert_eq!(Some("note: no tree row matches no/such/path"), app.status());
    }

    #[test]
    fn should_ignore_toggle_diff_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);
        assert_eq!(RightPane::Diff, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);

        assert_eq!(RightPane::Diff, app.right_pane());
        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );
    }

    #[test]
    fn should_return_none_diff_target_when_cursor_is_on_a_directory_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo")],
            }],
            ..empty_report()
        };
        let app = App::new(&report);

        let actual = app.selected_diff_target(&report);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_file_diff_target_when_cursor_is_on_a_file_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report);

        let actual = app.selected_diff_target(&report);

        assert_eq!(
            Some(DiffTarget::File {
                path: "lib.rs".to_string()
            }),
            actual
        );
    }

    #[test]
    fn should_return_file_diff_target_when_cursor_is_on_a_skipped_file_row() {
        // A skipped file has no symbols, but `selected_diff_target` scopes
        // a file row's diff to the whole file regardless of `skip_reason`
        // (only the entry-tree label/detail pane change for a skipped
        // file) — the raw diff hunks for it still exist and should still
        // be reachable via the diff pane.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            skipped: vec![rinkaku_core::render::SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            ..empty_report()
        };
        // Row 0 is the collapsing "assets" dir (single child, see
        // `crate::tree::build_tree`'s collapsing rule); row 1 is the
        // skipped "logo.png" file itself.
        let app = App::new(&report).handle_key(InputKey::Down);

        let actual = app.selected_diff_target(&report);

        assert_eq!(
            Some(DiffTarget::File {
                path: "assets/logo.png".to_string()
            }),
            actual
        );
    }

    #[test]
    fn should_return_symbol_diff_target_with_line_range_when_cursor_is_on_a_symbol_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    range: LineRange { start: 3, end: 7 },
                    ..symbol("lib.rs::foo", "foo")
                }],
            }],
            ..empty_report()
        };
        let app = App::new(&report).handle_key(InputKey::Down);

        let actual = app.selected_diff_target(&report);

        assert_eq!(
            Some(DiffTarget::Symbol {
                path: "lib.rs".to_string(),
                range_start: 3,
                range_end: 7,
            }),
            actual
        );
    }

    #[test]
    fn should_start_with_zero_right_pane_scroll() {
        let report = empty_report();
        let app = App::new(&report);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_start_with_tree_focus() {
        let report = empty_report();
        let app = App::new(&report);

        assert_eq!(Focus::Tree, app.focus());
    }

    #[test]
    fn should_move_focus_to_right_when_open_is_pressed_on_a_file_row() {
        let report = report_with_one_symbol();
        // Row 0 is the "lib.rs" file row itself (cursor starts there).
        let app = App::new(&report);

        let app = app.handle_key(InputKey::Open);

        assert_eq!(Focus::Right, app.focus());
        assert_eq!(Screen::Entry, *app.screen());
    }

    #[test]
    fn should_move_focus_to_right_and_open_source_when_open_is_pressed_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Down);

        let app = app.handle_key(InputKey::Open);

        assert_eq!(Focus::Right, app.focus());
        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );
    }

    #[test]
    fn should_expand_collapse_and_keep_tree_focus_when_open_is_pressed_on_a_directory_row() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo")],
            }],
            ..empty_report()
        };
        // Row 0 is the "src" directory itself.
        let app = App::new(&report);

        let app = app.handle_key(InputKey::Open);

        assert_eq!(Focus::Tree, app.focus());
        let rows = app.nav().rows(app.tree());
        let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
        assert_eq!(vec!["src"], paths, "directory should have collapsed");
    }

    #[test]
    fn should_not_move_focus_when_select_is_pressed_on_a_file_row() {
        // Space (`InputKey::Select`) must never move focus, even on a
        // file/symbol row — only Enter (`InputKey::Open`) does (ADR 0020).
        let report = report_with_one_symbol();
        let app = App::new(&report);

        let app = app.handle_key(InputKey::Select);

        assert_eq!(Focus::Tree, app.focus());
    }

    #[test]
    fn should_move_cursor_while_tree_focused_when_down_is_pressed() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        assert_eq!(Focus::Tree, app.focus());

        let app = app.handle_key(InputKey::Down);

        assert_eq!(1, app.nav().cursor());
    }

    #[test]
    fn should_scroll_right_pane_instead_of_moving_cursor_when_down_is_pressed_while_right_focused()
    {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Open); // focus -> Right
        let cursor_before = app.nav().cursor();

        let app = app.handle_key(InputKey::Down).handle_key(InputKey::Down);

        assert_eq!(cursor_before, app.nav().cursor());
        assert_eq!(2, app.right_pane_scroll());
    }

    #[test]
    fn should_decrement_right_pane_scroll_when_up_is_pressed_while_right_focused() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Open) // focus -> Right
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Down);
        assert_eq!(2, app.right_pane_scroll());

        let app = app.handle_key(InputKey::Up);

        assert_eq!(1, app.right_pane_scroll());
    }

    #[test]
    fn should_not_scroll_up_past_zero() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Open);

        let app = app.handle_key(InputKey::Up);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_return_focus_to_tree_when_focus_left_is_pressed_while_right_focused() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Open);
        assert_eq!(Focus::Right, app.focus());

        let app = app.handle_key(InputKey::FocusLeft);

        assert_eq!(Focus::Tree, app.focus());
    }

    #[test]
    fn should_do_nothing_when_focus_left_is_pressed_while_already_tree_focused() {
        let report = report_with_one_symbol();
        let app = App::new(&report);
        assert_eq!(Focus::Tree, app.focus());

        let app = app.handle_key(InputKey::FocusLeft);

        assert_eq!(Focus::Tree, app.focus());
    }

    #[test]
    fn should_start_with_help_overlay_closed() {
        let report = empty_report();
        let app = App::new(&report);

        assert_eq!(false, app.help_open());
    }

    #[test]
    fn should_open_help_overlay_when_toggle_help_is_pressed() {
        let report = empty_report();
        let app = App::new(&report);

        let app = app.handle_key(InputKey::ToggleHelp);

        assert_eq!(true, app.help_open());
    }

    #[test]
    fn should_close_help_overlay_when_toggle_help_is_pressed_again() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleHelp);
        assert_eq!(true, app.help_open());

        let app = app.handle_key(InputKey::ToggleHelp);

        assert_eq!(false, app.help_open());
    }

    #[test]
    fn should_ignore_quit_while_help_overlay_is_open() {
        // ADR 0020: the overlay must be a safe, low-stakes action that
        // cannot be short-circuited by an accidental app exit — `Quit`
        // reaching `handle_key` while the overlay is open (e.g. via a
        // translate_key bug) must still be swallowed defensively, not just
        // rely on `translate_key` never producing it in the first place.
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleHelp);
        assert_eq!(true, app.help_open());

        let app = app.handle_key(InputKey::Quit);

        assert_eq!(true, app.help_open());
        assert_eq!(false, app.should_quit());
    }

    #[test]
    fn should_ignore_navigation_keys_while_help_overlay_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::ToggleHelp);
        let cursor_before = app.nav().cursor();

        let app = app.handle_key(InputKey::Down);

        assert_eq!(cursor_before, app.nav().cursor());
        assert_eq!(true, app.help_open());
    }

    #[test]
    fn should_leave_other_state_untouched_when_help_overlay_opens() {
        // Opening the overlay must not disturb whatever was already showing
        // underneath it (`Self::help_open`'s own doc comment: "nothing else
        // about `App`'s state changes while the overlay is open").
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::ToggleDiff);
        let right_pane_before = app.right_pane();
        let cursor_before = app.nav().cursor();

        let app = app.handle_key(InputKey::ToggleHelp);

        assert_eq!(right_pane_before, app.right_pane());
        assert_eq!(cursor_before, app.nav().cursor());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_focus_returns_to_tree() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Open)
            .handle_key(InputKey::Down);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::FocusLeft);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_toggling_diff_pane() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Open)
            .handle_key(InputKey::Down);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::ToggleDiff);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_keep_right_pane_scroll_at_zero_when_returning_from_source_screen() {
        // Opening the source screen itself always resets scroll to 0
        // (`InputKey::Open`'s own reset, per the blanket rule) and every
        // key but `Back` is then a no-op while `Screen::Source` is active
        // (`App::handle_key`'s `Screen::Source` arm) — so scroll can never
        // become nonzero while the source screen is open in the first
        // place, unlike the pre-ADR-0020 world where `ScrollDown`/`ScrollUp`
        // were separate keys `Screen::Source`'s catch-all arm also
        // swallowed but which could still be pending from before entering.
        // This test pins that invariant end to end: `Back` finds scroll
        // already at 0 and leaves it there.
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
            .handle_key(InputKey::Open); // opens Screen::Source, focus -> Right
        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );
        assert_eq!(0, app.right_pane_scroll());

        let app = app.handle_key(InputKey::Back);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_ignore_scroll_keys_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Open); // opens source, focus -> Right

        let app = app.handle_key(InputKey::Down);

        assert_eq!(0, app.right_pane_scroll());
        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );
    }

    /// Two independent top-level directories, each with one file holding
    /// one symbol — deep/wide enough that `Nav::retarget_cursor` can land
    /// the cursor on a genuinely different node after a collapse, matching
    /// `nav.rs`'s own `should_not_move_cursor_when_collapse_happens_elsewhere_in_the_tree`
    /// fixture shape. Expanded row order: a(0), a/one.rs(1), foo(2), b(3),
    /// b/two.rs(4), bar(5).
    fn report_with_two_directories() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![
                FileReport {
                    path: "a/one.rs".to_string(),
                    symbols: vec![symbol("a/one.rs::foo", "foo")],
                },
                FileReport {
                    path: "b/two.rs".to_string(),
                    symbols: vec![symbol("b/two.rs::bar", "bar")],
                },
            ],
            ..empty_report()
        }
    }

    /// Moves the cursor down onto "a/one.rs" (a File row, row 1 of
    /// [`report_with_two_directories`]'s expanded order), presses `Open` to
    /// reach [`Focus::Right`] (ADR 0020: scrolling only applies there — a
    /// Dir row's own `Open` never changes focus, per `App::handle_key`'s
    /// `Open` arm, so this must land on a File/Symbol row specifically),
    /// then scrolls down by one line. Shared setup for every "does *this*
    /// action reset the scroll offset" test below, since
    /// `CollapseAll`/`ExpandAll`/`ToggleOrder` all remain tree-affecting
    /// regardless of which pane currently has focus (their `handle_key`
    /// match arms are focus-independent).
    fn focused_right_and_scrolled_one_line(app: App) -> App {
        app.handle_key(InputKey::Down)
            .handle_key(InputKey::Open)
            .handle_key(InputKey::Down)
    }

    #[test]
    fn should_reset_right_pane_scroll_when_select_collapses_the_row_under_the_cursor() {
        // Row 0 is "a" itself; collapsing it via `Select` hides its
        // children but the cursor's own row survives unmoved — still a
        // case the blanket reset rule must cover, since a directory row's
        // own detail content (fan-in/badges) does not depend on which of
        // its children are currently shown, but this pins the simplest
        // Select case regardless. `Open` on "a" (a directory row) does not
        // itself change focus (`App::handle_key`'s `Open` arm), so `Down`
        // right after it is still what actually reaches `Focus::Right` —
        // reusing the shared four-directory fixture below would change
        // which row is under the cursor, so this test builds its own
        // two-directory report and drives the two steps by hand instead of
        // via `focused_right_and_scrolled_one_line`.
        let report = report_with_two_directories();
        let app = App::new(&report);
        assert_eq!(Focus::Tree, app.focus());

        let app = app.handle_key(InputKey::Select);

        // `Select` never moves focus (ADR 0020), and scrolling never
        // applied here in the first place (Focus::Tree the whole time), so
        // this collapses to: collapsing "a" leaves the scroll offset at its
        // already-zero default. Kept as its own test (rather than folded
        // into a broader one) since it pins that `Select` specifically
        // never becomes a scroll-affecting action just because it can
        // reshuffle the row list, matching `CollapseAll`'s own case below.
        assert_eq!(0, app.right_pane_scroll());
        let rows = app.nav().rows(app.tree());
        let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
        // "bar" (a Symbol row) carries its containing file's path
        // ("b/two.rs"), not a path of its own (`TreeNode::path`'s own doc
        // comment) — so the flattened path list repeats "b/two.rs" for both
        // the File row and its one Symbol child.
        assert_eq!(vec!["a", "b", "b/two.rs", "b/two.rs"], paths);
    }

    #[test]
    fn should_reset_right_pane_scroll_when_collapse_all_retargets_cursor_onto_a_different_node() {
        // Cursor starts on "b/two.rs" (row 4, the File row directly under
        // "b"); CollapseAll hides every file/symbol row, and
        // `Nav::retarget_cursor` lands the cursor on "b" (the nearest
        // still-visible ancestor) — a genuinely different node's detail
        // than the one the pre-collapse scroll offset was scrolled into.
        // "b/two.rs" (a File row, not "bar"/a Symbol row) is the deliberate
        // choice: `Open` on a Symbol row also switches to `Screen::Source`
        // (`App::handle_key`'s `Open` arm), which would make the
        // `CollapseAll` this test presses next a no-op (every key but
        // `Back` is swallowed on `Screen::Source`) — a File row reaches
        // `Focus::Right` without leaving `Screen::Entry`.
        let report = report_with_two_directories();
        let mut app = App::new(&report);
        for _ in 0..4 {
            app = app.handle_key(InputKey::Down);
        }
        let rows = app.nav().rows(app.tree());
        assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
        let app = app
            .handle_key(InputKey::Open)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Down);
        assert_eq!(2, app.right_pane_scroll());

        let app = app.handle_key(InputKey::CollapseAll);

        let rows = app.nav().rows(app.tree());
        assert_eq!("b", rows[app.nav().cursor()].node.path);
        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_expand_all_is_pressed() {
        // `CollapseAll` first (before establishing focus/scroll) would land
        // the cursor on a Dir row ("a"), which `Open` cannot move focus from
        // (`App::handle_key`'s `Open` arm) — so this test instead reaches
        // Focus::Right + a nonzero scroll on "a/one.rs" while still
        // expanded, then presses `CollapseAll` followed by `ExpandAll` in
        // one breath and asserts the scroll is (still) 0 after both, which
        // is what actually matters: `ExpandAll` itself must never leave a
        // stale nonzero scroll behind, regardless of what `CollapseAll`
        // already reset it to just before.
        let report = report_with_two_directories();
        let app = focused_right_and_scrolled_one_line(App::new(&report));
        assert_eq!(1, app.right_pane_scroll());
        let app = app.handle_key(InputKey::CollapseAll);
        assert_eq!(0, app.right_pane_scroll());

        let app = app.handle_key(InputKey::ExpandAll);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_toggle_order_is_pressed() {
        // ToggleOrder can change which row now sits at the same cursor
        // index (reordering siblings), so it must reset the scroll offset
        // even though it never calls into `Nav` at all.
        let report = report_with_two_directories();
        let app = App::new(&report);
        let app = focused_right_and_scrolled_one_line(app);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::ToggleOrder);

        assert_eq!(0, app.right_pane_scroll());
    }
}
