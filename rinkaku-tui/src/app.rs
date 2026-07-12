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
/// iteration 2): the existing signature/used-by/callers detail, or the raw
/// diff hunks touching the selected row. Independent of [`Screen`] — it is
/// a display mode for the entry view's right pane, not a separate screen
/// reached via drill-down the way [`Screen::Source`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightPane {
    #[default]
    Detail,
    Diff,
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

    /// Applies one [`InputKey`] and returns the next `App`. `report` is
    /// needed only for [`InputKey::Source`] (to confirm the row under the
    /// cursor is a present symbol before switching screens — the actual
    /// file read happens later, in `crate::run`, once `Screen::Source` is
    /// active) and is otherwise unused.
    pub fn handle_key(mut self, key: InputKey) -> Self {
        self.status = None;

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
                    RightPane::Detail => RightPane::Diff,
                    RightPane::Diff => RightPane::Detail,
                };
            }
            (Screen::Entry, InputKey::Back) => {
                // No-op: Esc/q-as-back on the entry view has nowhere to
                // return to. Quitting from the entry view is the
                // dedicated `InputKey::Quit` variant instead.
            }
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
}
