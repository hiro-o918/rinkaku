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

use crate::detail::{DetailView, build_detail};
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

    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// The detail view for the row currently under the cursor, or `None`
    /// when the cursor is not on a present (non-removed) symbol row, there
    /// are no rows at all, or `report` no longer contains that symbol
    /// (defensive — `report` should be the same one `App::new` was built
    /// from). `report` is threaded in per call rather than stored on `App`
    /// itself, since `build_detail` is already a cheap pure lookup and
    /// storing a whole `Report` on every `App` would duplicate data the
    /// caller (`crate::run`) already owns for the process's lifetime.
    pub fn selected_detail(&self, report: &Report) -> Option<DetailView> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => {
                build_detail(report, &symbol_ref.id)
            }
            _ => None,
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
    fn should_return_none_detail_when_cursor_is_on_a_directory_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report);

        let actual = app.selected_detail(&report);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_detail_view_when_cursor_is_on_a_symbol_row() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(InputKey::Down);

        let actual = app.selected_detail(&report);

        assert_eq!("foo", actual.expect("detail for selected symbol").name);
    }
}
