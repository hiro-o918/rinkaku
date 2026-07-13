//! `ratatui` rendering (stage B, ADR 0015/0016): draws one frame from the
//! current [`App`] state. This is the crate's thin adapter layer — layout
//! decisions live here, but every value drawn (row text/style, detail
//! fields, source lines) comes from a pure view-model computed elsewhere
//! (`crate::app`, `crate::row_view`, `crate::detail`, `crate::source`).
//!
//! Kept deliberately un-unit-tested beyond the coarse `TestBackend`
//! snapshots in this module's own submodule test blocks (ADR 0016:
//! "rendering itself is covered separately... kept few and coarse — enough
//! to catch a broken layout, not to pin every pixel").

mod blast_radius;
mod detail_pane;
mod diff_pane;
mod entry;
mod overlay;
mod scroll;
mod source_screen;
mod status;
mod style;

use crate::app::{App, BlastRadiusSelection, Screen};
use crate::highlight::HighlightedFile;
use crate::source::HighlightedSourceView;
use entry::draw_entry_screen;
use overlay::{draw_help_overlay, draw_jump_popup};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use rinkaku_core::render::Report;
use source_screen::draw_source_screen;
use status::draw_status_line;

/// Everything [`crate::run_app`] needs to fold back into `App` after each
/// draw: the pane-scroll actually rendered this frame (so an overshot
/// request never survives past the visible clamp) and the inner height of
/// the currently-scrolling pane (so the next `Ctrl-d`/`Ctrl-u` half-page
/// step, ADR 0026, can be sized against the real viewport rather than a
/// magic constant) — plus the same pair again for the `?` help overlay,
/// which can be open and scrolling independently of whichever screen is
/// showing underneath it.
///
/// Every field is `Option<usize>` because a given frame may not have
/// anything to fold back at all — no right pane on [`Screen::Source`],
/// no scrolling pane at all when the tree has focus, the overlay closed,
/// etc. `crate::run_app` treats each `None` as "nothing to update on that
/// seam this frame", mirroring the pre-existing single-`Option<usize>`
/// return this replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrawOutcome {
    /// The right-hand pane's scroll offset as actually clamped and
    /// rendered this frame (`None` on [`Screen::Source`], which scrolls
    /// via its own `Screen::Source::scroll_top` rather than
    /// `App::right_pane_scroll`, or when the active right pane rendered
    /// a placeholder with nothing to scroll — `draw_entry_screen`'s own
    /// doc comment). `crate::run_app` feeds this back into `App`
    /// (`App::with_right_pane_scroll`) after every draw so an overshot
    /// scroll request (dogfooding finding: repeated `j` past the
    /// content's end used to keep incrementing `App`'s unclamped
    /// "requested" scroll with no visible effect, so winding back down
    /// again took as many `k` presses as it took to overshoot) never
    /// survives past the frame that visibly clamped it.
    pub clamped_right_pane_scroll: Option<usize>,
    /// The inner height (borders excluded) of whichever pane is currently
    /// scrollable — the source pane on [`Screen::Source`], the right pane
    /// on [`Screen::Entry`] + [`crate::app::Focus::Right`], and `None`
    /// otherwise (Tree-focused on the entry view has no scrollable pane
    /// receiving motion). `crate::run_app` remembers this between frames
    /// and passes it into [`crate::app::App::handle_scroll_key`] when a
    /// half-page (ADR 0026) key arrives, so the step size scales with
    /// the actual pane rather than a magic constant.
    pub scroll_viewport_height: Option<usize>,
    /// The `?` help overlay's own scroll offset as actually clamped and
    /// rendered this frame (`None` while the overlay is closed) — kept
    /// separate from [`Self::clamped_right_pane_scroll`] rather than reusing
    /// it, since the overlay composites *on top of* whichever screen was
    /// already showing (`crate::app::App::help_open`'s own doc comment) and
    /// can be open while that underlying screen still has its own scroll
    /// state to fold back independently; collapsing the two into one field
    /// would make one clobber the other the moment both are scrollable in
    /// the same frame. `crate::run_app` feeds this back via
    /// `App::with_help_scroll`, mirroring the right-pane fold-back.
    pub clamped_help_scroll: Option<usize>,
    /// The help overlay's own inner height (borders excluded) this frame,
    /// `None` while closed — `crate::run_app` remembers this the same way
    /// it does [`Self::scroll_viewport_height`], so a `Ctrl-d`/`Ctrl-u`
    /// half-page press while the overlay is open sizes its step against the
    /// overlay's own box, not whatever pane happens to be underneath it.
    pub help_scroll_viewport_height: Option<usize>,
}

/// Draws one full frame: the entry view (tree + right pane split) or the
/// source drill-down, depending on `app.screen()`, with a status/help line
/// pinned to the bottom either way. `diff_highlights` is the whole diff's
/// per-line syntax highlighting, computed once by `crate::run_app` (not
/// re-parsed/re-highlighted here on every frame — see that function's doc
/// comment on why that work lives outside the draw loop), consulted only
/// when the right pane is in [`crate::app::RightPane::Diff`] mode. `diff_content` and
/// `blast_radius_selection` are likewise computed once per handled key by
/// `crate::run_app` (not here), for [`crate::app::RightPane::Diff`]/[`crate::app::RightPane::BlastRadius`]
/// respectively — see `App::selected_blast_radius_view`'s own doc comment on why
/// this function must not call either computation itself. `source_content` is
/// the same discipline applied to [`Screen::Source`]: `crate::run_app` calls
/// [`crate::source::load_highlighted_symbol_source`] exactly once, when the
/// `s` key opens the screen (a file read plus a full tree-sitter parse — ADR
/// 0018's own "must not run inside the render loop" rule, extended from the
/// diff pane to this screen), and hands the `Result` here unchanged on every
/// subsequent draw rather than this function re-reading/re-highlighting the
/// file itself. `None` only when [`Screen::Source`] is not the active screen
/// (`crate::run_app` has nothing to compute yet).
///
/// The `?` help overlay (ADR 0020) draws last, on top of whatever screen
/// was already rendered underneath — `app.help_open()` never changes what
/// `Screen`/`crate::app::RightPane` themselves draw (`App::help_open`'s own doc
/// comment), so the underlying frame is built exactly the same way whether
/// the overlay is open or not, and the overlay is simply composited over
/// it as a final step. Its own clamped scroll offset and inner height
/// (scrolling, added after the overlay's content outgrew a single fixed
/// box) are folded into `outcome` after the base match above, rather than
/// as another arm of it, since the overlay is orthogonal to which `Screen`
/// is underneath — `outcome`'s `clamped_right_pane_scroll`/
/// `scroll_viewport_height` pair (whichever screen set it) is left
/// untouched by this step.
pub fn draw(
    frame: &mut Frame,
    app: &App,
    report: &Report,
    diff_content: &crate::diff_shape::DiffPaneContent,
    diff_highlights: &[HighlightedFile],
    blast_radius_selection: &BlastRadiusSelection,
    source_content: Option<&Result<HighlightedSourceView, String>>,
) -> DrawOutcome {
    let area = frame.area();
    let [body, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);

    let mut outcome = match app.screen() {
        Screen::Entry => {
            let clamped = draw_entry_screen(
                frame,
                app,
                report,
                diff_content,
                diff_highlights,
                blast_radius_selection,
                body,
            );
            // Right-pane inner height (borders excluded), computed the
            // same way `draw_entry_screen`'s split does when it actually
            // renders — mirrored here so the scroll pipeline sees the
            // very same viewport the reviewer just saw. Only reported
            // while `Focus::Right` (the only state where scroll keys
            // apply to the right pane); a Tree-focused frame has no
            // scrolling pane to size, so returns `None`.
            let scroll_viewport_height = if app.focus() == crate::app::Focus::Right {
                Some(right_pane_viewport_height(body))
            } else {
                None
            };
            DrawOutcome {
                clamped_right_pane_scroll: clamped,
                scroll_viewport_height,
                clamped_help_scroll: None,
                help_scroll_viewport_height: None,
            }
        }
        Screen::Source {
            symbol_id,
            scroll_top,
        } => {
            let inner_height = body.height.saturating_sub(2) as usize;
            draw_source_screen(frame, symbol_id, *scroll_top, source_content, body);
            DrawOutcome {
                clamped_right_pane_scroll: None,
                scroll_viewport_height: Some(inner_height),
                clamped_help_scroll: None,
                help_scroll_viewport_height: None,
            }
        }
    };

    draw_status_line(frame, app, report, status_area);

    if app.help_open() {
        let (clamped_help_scroll, help_scroll_viewport_height) =
            draw_help_overlay(frame, area, app.help_scroll());
        outcome.clamped_help_scroll = Some(clamped_help_scroll);
        outcome.help_scroll_viewport_height = Some(help_scroll_viewport_height);
    }
    if let Some(popup) = app.jump_popup() {
        draw_jump_popup(frame, popup, area);
    }

    outcome
}

/// The right pane's inner height (borders excluded), computed from `body`
/// (the entry screen's full body area) using the same 60/40 horizontal
/// split and `saturating_sub(2)` border deduction [`draw_entry_screen`]
/// itself uses (`ENTRY_TREE_WIDTH_PERCENT`/`ENTRY_RIGHT_WIDTH_PERCENT`
/// below, referenced both here and there so the two never drift).
/// Extracted so [`DrawOutcome`]'s `scroll_viewport_height` reflects the
/// exact viewport a reviewer just saw.
fn right_pane_viewport_height(body: Rect) -> usize {
    let [_, right] = Layout::horizontal([
        Constraint::Percentage(ENTRY_TREE_WIDTH_PERCENT),
        Constraint::Percentage(ENTRY_RIGHT_WIDTH_PERCENT),
    ])
    .areas(body);
    right.height.saturating_sub(2) as usize
}

/// Tree-pane / right-pane horizontal split percentages for [`Screen::Entry`],
/// shared between [`draw_entry_screen`]'s actual layout and
/// [`right_pane_viewport_height`]'s report of that layout to
/// [`DrawOutcome::scroll_viewport_height`] (ADR 0026) so the two cannot
/// drift.
pub(crate) const ENTRY_TREE_WIDTH_PERCENT: u16 = 60;
pub(crate) const ENTRY_RIGHT_WIDTH_PERCENT: u16 = 40;
