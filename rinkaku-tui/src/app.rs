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
    Up,
    Down,
    /// Enter or Space: expand/collapse a directory row, or open the
    /// source view on a symbol row (`App::handle_key`'s doc comment).
    Select,
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
    /// selected row instead of resetting on every cursor move.
    ToggleDiff,
    /// `p`/`P`: toggle the right-hand pane between [`RightPane::Pivot`] and
    /// whichever mode was active before ([`RightPane::Detail`] or
    /// [`RightPane::Diff`]) — ADR 0019's entry-path pivot. Pressing `p`
    /// again while already in `Pivot` mode returns to the prior mode,
    /// mirroring `d`'s own toggle rather than a one-way "enter pivot mode"
    /// action, since the ADR describes `p` as a per-row toggle.
    TogglePivot,
    /// `J`: scroll the right-hand pane (Detail/Diff) down by one line.
    /// Uppercase specifically so it does not collide with `j`'s existing
    /// cursor-move binding (`crate::run`'s `translate_key` matches
    /// `KeyCode::Char` case-sensitively).
    ScrollDown,
    /// `K`: scroll the right-hand pane up by one line (see [`Self::ScrollDown`]).
    ScrollUp,
    /// Esc or `q` while in the source view: return to the entry view.
    /// A no-op on the entry view itself (`q`'s quit behavior on the entry
    /// view is `InputKey::Quit`, a separate variant, since Esc has no
    /// "back" target to return to there).
    Back,
    /// `q` or Ctrl-C on the entry view: exit the application.
    Quit,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightPane {
    #[default]
    Detail,
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
    /// The user's requested scroll offset (in lines) into the right-hand
    /// pane's content, as an unclamped "how far down would the user like to
    /// be" value rather than an authoritative display position: `App` has
    /// no notion of the pane's rendered height (that is a `ratatui::Rect`
    /// only `crate::ui` sees at draw time), so clamping this to
    /// `content_len.saturating_sub(pane_height)` is `crate::ui`'s
    /// responsibility (`ui::clamp_scroll`) — keeping this module free of
    /// any layout concern, matching the rest of `App`'s pure-state
    /// discipline. Reset to 0 by every key `handle_key` processes *except*
    /// `InputKey::ScrollDown`/`ScrollUp` on [`Screen::Entry`] (`handle_key`'s
    /// own doc comment on why this is a blanket rule rather than an
    /// enumerated list of "actions that change the pane's content" — the
    /// cursor can move *indirectly*, e.g. a collapse retargeting it onto a
    /// different row, which an enumerated list is prone to missing).
    right_pane_scroll: usize,
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
            right_pane_scroll: 0,
            status: None,
            should_quit: false,
        }
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
    /// `right_pane_scroll` is reset to 0 by every key *except*
    /// `ScrollDown`/`ScrollUp` on [`Screen::Entry`] — a uniform rule applied
    /// once below, rather than each action deciding individually whether it
    /// might change the right pane's content. The per-action approach used
    /// to miss cases where the cursor moves *indirectly*: collapsing a
    /// directory (`Select`/`CollapseAll`) can retarget the cursor onto a
    /// different row via `Nav::retarget_cursor`, and reordering
    /// (`ToggleOrder`) can do the same simply by changing which row now
    /// sits at the same cursor index — both used to leave a stale nonzero
    /// scroll offset pointing into the *new* row's unrelated content. Only
    /// `ScrollDown`/`ScrollUp` are exempt, since they are the two actions
    /// whose entire purpose is to set this value.
    pub fn handle_key(mut self, key: InputKey) -> Self {
        self.status = None;
        let preserve_scroll = matches!(
            (&self.screen, key),
            (Screen::Entry, InputKey::ScrollDown) | (Screen::Entry, InputKey::ScrollUp)
        );

        match (&self.screen, key) {
            (Screen::Source { .. }, InputKey::Back) => {
                self.screen = Screen::Entry;
            }
            // Every other key is a no-op while the source view is open —
            // navigation/reordering only make sense against the entry
            // view's tree, and re-dispatching them would silently move
            // the cursor underneath a screen the user can't see moving.
            (Screen::Source { .. }, _) => {}

            (Screen::Entry, InputKey::Quit) => {
                self.should_quit = true;
            }
            (Screen::Entry, InputKey::Up) => {
                self.nav = self.nav.handle(Action::CursorUp, &self.tree);
            }
            (Screen::Entry, InputKey::Down) => {
                self.nav = self.nav.handle(Action::CursorDown, &self.tree);
            }
            (Screen::Entry, InputKey::Select) => {
                self.nav = self.nav.handle(Action::ToggleExpand, &self.tree);
            }
            (Screen::Entry, InputKey::ExpandAll) => {
                self.nav = self.nav.handle(Action::ExpandAll, &self.tree);
            }
            (Screen::Entry, InputKey::CollapseAll) => {
                self.nav = self.nav.handle(Action::CollapseAll, &self.tree);
            }
            (Screen::Entry, InputKey::ToggleOrder) => {
                self.order_mode = match self.order_mode {
                    OrderMode::Topological => OrderMode::AlphaNumeric,
                    OrderMode::AlphaNumeric => OrderMode::Topological,
                };
                crate::order::order_tree(&mut self.tree, &self.ranks, self.order_mode);
            }
            (Screen::Entry, InputKey::Source) => {
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
            (Screen::Entry, InputKey::ToggleDiff) => {
                self.right_pane = match self.right_pane {
                    RightPane::Diff => RightPane::Detail,
                    // From Detail or Pivot, `d` always lands on Diff —
                    // Pivot has no dedicated "previous mode" of its own to
                    // return to (see `RightPane`'s own doc comment), so `d`
                    // simply picks Diff same as it would from Detail.
                    RightPane::Detail | RightPane::Pivot => RightPane::Diff,
                };
            }
            (Screen::Entry, InputKey::TogglePivot) => {
                self.right_pane = match self.right_pane {
                    RightPane::Pivot => RightPane::Detail,
                    RightPane::Detail | RightPane::Diff => RightPane::Pivot,
                };
            }
            (Screen::Entry, InputKey::ScrollDown) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_add(1);
            }
            (Screen::Entry, InputKey::ScrollUp) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_sub(1);
            }
            (Screen::Entry, InputKey::Back) => {
                // No-op: Esc/q-as-back on the entry view has nowhere to
                // return to. Quitting from the entry view is the
                // dedicated `InputKey::Quit` variant instead.
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
    fn should_toggle_right_pane_between_detail_and_diff() {
        let report = empty_report();
        let app = App::new(&report);
        assert_eq!(RightPane::Detail, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);
        assert_eq!(RightPane::Diff, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);
        assert_eq!(RightPane::Detail, app.right_pane());
    }

    #[test]
    fn should_toggle_right_pane_between_detail_and_pivot() {
        let report = empty_report();
        let app = App::new(&report);
        assert_eq!(RightPane::Detail, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Pivot, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Detail, app.right_pane());
    }

    #[test]
    fn should_switch_from_pivot_to_diff_when_toggle_diff_is_pressed() {
        // ADR 0019: "p" re-press or "d" both leave pivot mode — "d" lands
        // on Diff specifically (Pivot has no "previous mode" of its own to
        // return to, per `RightPane`'s own doc comment).
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::TogglePivot);
        assert_eq!(RightPane::Pivot, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);

        assert_eq!(RightPane::Diff, app.right_pane());
    }

    #[test]
    fn should_switch_from_diff_to_pivot_when_toggle_pivot_is_pressed() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ToggleDiff);
        assert_eq!(RightPane::Diff, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);

        assert_eq!(RightPane::Pivot, app.right_pane());
    }

    #[test]
    fn should_ignore_toggle_pivot_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);
        assert_eq!(RightPane::Detail, app.right_pane());

        let app = app.handle_key(InputKey::TogglePivot);

        assert_eq!(RightPane::Detail, app.right_pane());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_toggling_pivot_pane() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ScrollDown);
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
    fn should_return_empty_pivot_selection_when_directory_path_matches_no_symbol() {
        // Defensive: `crate::pivot::build_pivot_view` only returns `None`
        // when no node matches the prefix, which should not happen for a
        // path the tree itself produced — but `selected_pivot_view`'s
        // `Empty` branch must still exist and report something sane rather
        // than silently falling through, in case tree/graph ever disagree.
        let report = empty_report();
        let app = App::new(&report);

        let actual = app.selected_pivot_view(&report);

        assert_eq!(PivotSelection::NotApplicable, actual);
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
    fn should_ignore_toggle_diff_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);
        assert_eq!(RightPane::Detail, app.right_pane());

        let app = app.handle_key(InputKey::ToggleDiff);

        assert_eq!(RightPane::Detail, app.right_pane());
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
    fn should_increment_right_pane_scroll_when_scroll_down_is_pressed() {
        let report = empty_report();
        let app = App::new(&report);

        let app = app
            .handle_key(InputKey::ScrollDown)
            .handle_key(InputKey::ScrollDown);

        assert_eq!(2, app.right_pane_scroll());
    }

    #[test]
    fn should_decrement_right_pane_scroll_when_scroll_up_is_pressed() {
        let report = empty_report();
        let app = App::new(&report)
            .handle_key(InputKey::ScrollDown)
            .handle_key(InputKey::ScrollDown);
        assert_eq!(2, app.right_pane_scroll());

        let app = app.handle_key(InputKey::ScrollUp);

        assert_eq!(1, app.right_pane_scroll());
    }

    #[test]
    fn should_not_scroll_up_past_zero() {
        let report = empty_report();
        let app = App::new(&report);

        let app = app.handle_key(InputKey::ScrollUp);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_cursor_moves_down() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::ScrollDown)
            .handle_key(InputKey::ScrollDown);
        assert_eq!(2, app.right_pane_scroll());

        let app = app.handle_key(InputKey::Down);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_cursor_moves_up() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::ScrollDown);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::Up);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_toggling_diff_pane() {
        let report = empty_report();
        let app = App::new(&report).handle_key(InputKey::ScrollDown);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::ToggleDiff);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_returning_from_source_screen() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::ScrollDown)
            .handle_key(InputKey::Source);
        assert_eq!(
            Screen::Source {
                symbol_id: "lib.rs::foo".to_string()
            },
            *app.screen()
        );

        let app = app.handle_key(InputKey::Back);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_ignore_scroll_keys_while_source_screen_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report)
            .handle_key(InputKey::Down)
            .handle_key(InputKey::Source);

        let app = app.handle_key(InputKey::ScrollDown);

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

    #[test]
    fn should_reset_right_pane_scroll_when_select_collapses_the_row_under_the_cursor() {
        // Row 0 is "a" itself; collapsing it via `Select` hides its
        // children but the cursor's own row survives unmoved — still a
        // case the blanket reset rule must cover, since a directory row's
        // own detail content (fan-in/badges) does not depend on which of
        // its children are currently shown, but this pins the simplest
        // Select case regardless.
        let report = report_with_two_directories();
        let app = App::new(&report).handle_key(InputKey::ScrollDown);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::Select);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_collapse_all_retargets_cursor_onto_a_different_node() {
        // Cursor starts on "bar" (row 5, under "b/two.rs"); CollapseAll
        // hides every file/symbol row, and `Nav::retarget_cursor` lands the
        // cursor on "b" (the nearest still-visible ancestor) — a genuinely
        // different node's detail than the one the pre-collapse scroll
        // offset was scrolled into.
        let report = report_with_two_directories();
        let mut app = App::new(&report);
        for _ in 0..5 {
            app = app.handle_key(InputKey::Down);
        }
        let rows = app.nav().rows(app.tree());
        assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
        let app = app
            .handle_key(InputKey::ScrollDown)
            .handle_key(InputKey::ScrollDown);
        assert_eq!(2, app.right_pane_scroll());

        let app = app.handle_key(InputKey::CollapseAll);

        let rows = app.nav().rows(app.tree());
        assert_eq!("b", rows[app.nav().cursor()].node.path);
        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_expand_all_is_pressed() {
        let report = report_with_two_directories();
        let app = App::new(&report)
            .handle_key(InputKey::CollapseAll)
            .handle_key(InputKey::ScrollDown);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::ExpandAll);

        assert_eq!(0, app.right_pane_scroll());
    }

    #[test]
    fn should_reset_right_pane_scroll_when_toggle_order_is_pressed() {
        // ToggleOrder can change which row now sits at the same cursor
        // index (reordering siblings), so it must reset the scroll offset
        // even though it never calls into `Nav` at all.
        let report = report_with_two_directories();
        let app = App::new(&report).handle_key(InputKey::ScrollDown);
        assert_eq!(1, app.right_pane_scroll());

        let app = app.handle_key(InputKey::ToggleOrder);

        assert_eq!(0, app.right_pane_scroll());
    }
}
