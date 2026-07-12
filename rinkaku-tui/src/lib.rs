//! rinkaku's interactive terminal UI (ADR 0015/0016).
//!
//! Two layers, kept deliberately separate:
//!
//! - **View-models** (`tree`, `nav`, `order`, `detail`, `app`, `row_view`):
//!   plain data and pure functions/state machines derived from
//!   [`rinkaku_core::render::Report`]. `tree`/`nav`/`order`/`detail` carry
//!   no `ratatui`/`crossterm` types at all (ADR 0016 decision 3). `app`
//!   and `row_view` are the stage B additions that compose those
//!   view-models into one navigable state machine and format its rows —
//!   `row_view` uses `ratatui::text`/`style` types (`Line`/`Span`/`Style`),
//!   which are plain, comparable data rather than a live `Frame`/
//!   `Terminal`, so building one from a row stays a pure, unit-testable
//!   transformation. `app` stays entirely free of `ratatui`/`crossterm`
//!   types, translating real key events at the boundary instead (see
//!   `run`'s event loop).
//! - **Terminal adapter** (`ui`, `source`, [`run`]): draws `App`'s state
//!   with `ratatui`, reads source files for the drill-down view, and owns
//!   the terminal lifecycle (raw mode, alternate screen, the event loop).
//!   This is the only layer that performs IO or holds a live `Terminal`.
//!
//! [`run`] is the crate's single public entry point for the CLI binary:
//! `rinkaku`'s `main.rs` hands it a [`rinkaku_core::render::Report`] once
//! `--tui` is passed, in place of rendering Markdown/JSON.

pub mod app;
pub mod detail;
pub mod nav;
pub mod order;
pub mod row_view;
pub mod source;
pub mod tree;
pub mod ui;

use app::{App, InputKey, Screen};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use rinkaku_core::render::Report;
use std::time::Duration;

/// Runs the interactive TUI over `report` until the user quits, taking
/// over the terminal for the duration of the call (raw mode + alternate
/// screen via [`ratatui::try_init`], restored on return **and** on panic —
/// `ratatui::try_init`'s own panic hook covers the latter, so a bug in this
/// crate cannot leave the caller's terminal in raw mode).
///
/// Uses `try_init` rather than [`ratatui::init`] specifically so terminal
/// setup failure (e.g. stdin/stdout is not a TTY at all — piped input,
/// `< /dev/null`, a CI runner) surfaces as an `Err` for `main.rs`'s
/// `anyhow` path to print cleanly and exit 1, instead of `ratatui::init`'s
/// own `.expect(...)` panicking with a raw Rust panic message and exit
/// code 101.
///
/// This is the only function in the crate that touches a real terminal or
/// blocks on input; everything it calls into (`App`, `row_view`, `ui`,
/// `source`) is either pure or an isolated, narrowly-scoped IO call (a
/// single source-file read).
pub fn run(report: &Report) -> std::io::Result<()> {
    let mut terminal = ratatui::try_init()?;
    let result = run_app(&mut terminal, report);
    ratatui::restore();
    result
}

fn run_app(terminal: &mut ratatui::DefaultTerminal, report: &Report) -> std::io::Result<()> {
    let mut app = App::new(report);

    loop {
        terminal.draw(|frame| ui::draw(frame, &app, report))?;

        if app.should_quit() {
            return Ok(());
        }

        // A 100ms poll timeout keeps the loop responsive to terminal
        // resize events without busy-spinning — `event::read()` alone
        // would block indefinitely on a genuinely idle terminal, which is
        // fine for input but would also delay reacting to anything else
        // this loop might grow to check in the future (kept short as a
        // resize/redraw responsiveness margin, not a correctness
        // requirement of the key-handling itself).
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key_event) = event::read()?
            && key_event.kind == KeyEventKind::Press
            && let Some(input_key) = translate_key(key_event.code, key_event.modifiers, &app)
        {
            if let InputKey::Source = input_key {
                app = app.handle_key(input_key);
                if let Screen::Source { symbol_id } = app.screen().clone() {
                    match source::load_symbol_source(report, &symbol_id) {
                        // The `SourceView` itself is discarded here — only
                        // used to detect a failure early so it can be
                        // surfaced on the status line right away, rather
                        // than silently on the next redraw. `ui::draw`'s
                        // `draw_source_screen` re-reads the file itself
                        // when it renders the screen (see that function's
                        // doc comment for why it re-reads instead of
                        // caching this result).
                        Ok(_) => {}
                        Err(message) => app.set_status(message),
                    }
                }
            } else {
                app = app.handle_key(input_key);
            }
        }
    }
}

/// Translates a raw `crossterm` key press into this crate's
/// terminal-agnostic [`InputKey`], or `None` for a key the app does not
/// react to. Depends on `app.screen()` only to disambiguate `q`/Esc
/// (`Quit` on the entry view, `Back` on the source view) — every other
/// mapping is context-free.
fn translate_key(code: KeyCode, modifiers: KeyModifiers, app: &App) -> Option<InputKey> {
    let on_source_screen = matches!(app.screen(), Screen::Source { .. });

    match code {
        KeyCode::Up | KeyCode::Char('k') => Some(InputKey::Up),
        KeyCode::Down | KeyCode::Char('j') => Some(InputKey::Down),
        KeyCode::Enter | KeyCode::Char(' ') => Some(InputKey::Select),
        KeyCode::Char('e') | KeyCode::Char('E') => Some(InputKey::ExpandAll),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => Some(InputKey::Quit),
        KeyCode::Char('c') | KeyCode::Char('C') => Some(InputKey::CollapseAll),
        KeyCode::Char('o') | KeyCode::Char('O') => Some(InputKey::ToggleOrder),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(InputKey::Source),
        KeyCode::Esc if on_source_screen => Some(InputKey::Back),
        KeyCode::Char('q') if on_source_screen => Some(InputKey::Back),
        KeyCode::Char('q') => Some(InputKey::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rinkaku_core::graph::SymbolGraph;

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

    #[test]
    fn should_translate_ctrl_c_to_quit_regardless_of_screen() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('c'), KeyModifiers::CONTROL, &app);

        assert_eq!(Some(InputKey::Quit), actual);
    }

    #[test]
    fn should_translate_q_to_quit_on_entry_screen() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Char('q'), KeyModifiers::NONE, &app);

        assert_eq!(Some(InputKey::Quit), actual);
    }

    #[test]
    fn should_translate_esc_to_none_on_entry_screen() {
        // Esc has no "back" target on the entry screen (App::handle_key's
        // own doc comment) and is not bound to quit there either — quit is
        // 'q'/Ctrl-C only, so Esc is simply not handled at this screen.
        let report = empty_report();
        let app = App::new(&report);

        let actual = translate_key(KeyCode::Esc, KeyModifiers::NONE, &app);

        assert_eq!(None, actual);
    }
}
