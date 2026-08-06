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

mod handle_key;
mod input_key;
mod jump;
mod selection;
mod state;

pub use input_key::InputKey;
pub use jump::{JumpCandidate, JumpPopup, PendingPrefix};
pub(crate) use selection::SelectedRow;
pub use selection::{BlastRadiusSelection, DiffFocus, DiffTarget, SelectedDetail};
pub use state::App;

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
