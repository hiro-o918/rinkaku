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
//!   [`event_loop::run_app`]'s own event loop delegates responsibilities to
//!   sibling modules (ADR 0028, split out once this crate's event-loop code
//!   grew past the file-size threshold): `input_translate` (raw `crossterm`
//!   events → [`app::InputKey`]) and `review_flow` (ADR 0048's review-notes
//!   composing/exporting/caching glue).
//!
//! [`run`] is the crate's single public entry point for the CLI binary:
//! `rinkaku`'s `main.rs` hands it a [`rinkaku_core::render::Report`] once
//! `--tui` is passed, in place of rendering Markdown/JSON. It also hands in
//! the raw unified diff text `main.rs` already has in hand for every input
//! mode (stdin / `--base` / `--pr`) — TUI iteration 2's diff pane
//! (`d`/`D`, `crate::diff_view`) slices hunks straight out of that same
//! string rather than reconstructing a diff from `Report` (which no longer
//! carries hunk text once extraction has run).

// ADR 0055: the `?` help overlay's translations, compiled in from
// `locales/{en,ja}.yml` at build time. English is this macro's fallback,
// matching this project's fixed English default.
rust_i18n::i18n!("locales", fallback = "en");

pub mod app;
pub mod blast_radius;
pub mod detail;
pub mod diff_shape;
pub mod diff_view;
mod event_loop;
pub mod help;
pub mod highlight;
mod hunk_split;
mod input_translate;
pub mod locale;
pub mod nav;
pub mod note_markers;
pub mod order;
pub mod review;
mod review_flow;
pub mod row_view;
pub mod search;
mod session;
pub mod source;
pub mod source_diff;
pub mod source_split;
pub mod splash;
mod split_pairing;
pub mod tree;
pub mod ui;

pub use event_loop::ReviewPorts;
pub use session::{TuiSession, run};

pub(crate) use event_loop::run_app;
