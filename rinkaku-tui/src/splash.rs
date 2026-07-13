//! The startup splash screen (ADR 0033): the rinkaku logo plus the current
//! analysis phase, shown while `main.rs` runs the pre-render pipeline
//! synchronously on the same thread that already owns the terminal — see
//! [`crate::TuiSession`] for how a caller drives this alongside the
//! pipeline.
//!
//! Split the same way every other view in this crate is (module doc
//! comment, `crate` root): [`SplashState`] is plain data with no
//! `ratatui`/`crossterm` types, and [`draw_splash`] is the thin terminal
//! adapter that lays it out — mirroring `crate::ui`'s own
//! "view-model here, drawing there" split.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph};

/// The rinkaku wordmark, hardcoded rather than generated (ADR 0033
/// decision 3 — a static `const`, mirroring `crate::help::HelpContent`'s own
/// "fixed content, not computed" precedent). Kept short and wide rather
/// than tall, so it still fits above the phase label on a modest terminal
/// height (e.g. a CI pty running at 24 rows).
pub const LOGO_LINES: &[&str] = &[
    r"       _       _         _            ",
    r"  _ __(_)_ __ | | ____ _| | ___   _   ",
    r" | '__| | '_ \| |/ / _` | |/ / | | |  ",
    r" | |  | | | | |   < (_| |   <| |_| |  ",
    r" |_|  |_|_| |_|_|\_\__,_|_|\_\\__,_|  ",
];

/// The splash screen's pure view-model: which phase label to show under the
/// logo, and — only for the two file-scanning phases that can measure real
/// progress ([`rinkaku_core::pipeline::analyze_repo`],
/// [`rinkaku_core::deps::TagsResolver::new`]) — a `(done, total)` pair to
/// render as a determinate bar. `None` means "no real signal for this
/// phase", which [`draw_splash`] renders as a label with no bar rather than
/// a fake/simulated animation (ADR 0033 decision 3: "no fake progress").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashState {
    pub phase_label: String,
    pub progress: Option<(usize, usize)>,
}

impl SplashState {
    /// A label-only state with no progress bar — every phase except the
    /// two file-scanning ones (`--pr`/`--base` resolution, diffing,
    /// analyzing the diff's own changed symbols).
    pub fn label_only(phase_label: impl Into<String>) -> Self {
        Self {
            phase_label: phase_label.into(),
            progress: None,
        }
    }

    /// A state with a determinate progress bar — `analyze_repo`'s parallel
    /// parse or `TagsResolver::new`'s sequential index build, both of which
    /// know their total file count up front.
    pub fn with_progress(phase_label: impl Into<String>, done: usize, total: usize) -> Self {
        Self {
            phase_label: phase_label.into(),
            progress: Some((done, total)),
        }
    }
}

/// Draws one splash frame: the logo centered in the upper portion of the
/// screen, the phase label beneath it, and — when [`SplashState::progress`]
/// is `Some`, ADR 0033 decision 3 — a determinate [`Gauge`] beneath the
/// label showing `done`/`total` as both a filled bar and a `"{done}/{total}"`
/// label. Deliberately uncovered by unit tests beyond the coarse
/// `TestBackend` snapshot in this module's own test block, matching
/// `crate::ui::draw`'s own "rendering itself is covered separately... kept
/// few and coarse" precedent — there is no per-pixel behavior here worth
/// pinning beyond "the logo and the current phase both show up".
pub fn draw_splash(frame: &mut Frame, state: &SplashState) {
    let area = frame.area();

    let logo_height = LOGO_LINES.len() as u16;
    // Logo + one blank line + phase label + (optional) gauge, vertically
    // centered as a block rather than each line individually — a
    // fixed-height `Constraint::Length` block sized to exactly what this
    // frame needs, with `Constraint::Fill` above/below splitting the
    // remaining space evenly so the block sits in the middle regardless of
    // terminal height.
    let content_height = logo_height + 1 + 1 + if state.progress.is_some() { 1 } else { 0 };
    let [_, content, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(content_height),
        Constraint::Fill(1),
    ])
    .areas(area);

    let mut constraints = vec![Constraint::Length(logo_height), Constraint::Length(1)];
    if state.progress.is_some() {
        constraints.push(Constraint::Length(1));
    }
    let rows = Layout::vertical(constraints).split(content);

    let logo_lines: Vec<Line> = LOGO_LINES
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::default().fg(Color::Cyan))).centered())
        .collect();
    frame.render_widget(Paragraph::new(logo_lines), rows[0]);

    let label_line = Line::from(Span::styled(
        state.phase_label.clone(),
        Style::default().fg(Color::White),
    ))
    .alignment(Alignment::Center);
    frame.render_widget(Paragraph::new(label_line), rows[1]);

    if let Some((done, total)) = state.progress {
        let ratio = progress_ratio(done, total);
        let gauge_area = centered_gauge_area(rows[2]);
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(ratio)
            .label(format!("{done}/{total}"));
        frame.render_widget(gauge, gauge_area);
    }
}

/// Narrows `area` to a centered horizontal band for the gauge — the full
/// terminal width looks disproportionately wide for a single progress bar
/// sitting under a comparatively narrow logo/label, so this caps it at
/// [`GAUGE_WIDTH`] columns (or the full area, whichever is narrower, for a
/// terminal too small to fit that).
fn centered_gauge_area(area: Rect) -> Rect {
    let width = GAUGE_WIDTH.min(area.width);
    let [_, gauge, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(area);
    gauge
}

const GAUGE_WIDTH: u16 = 40;

/// `done / total` clamped into `Gauge::ratio`'s required `0.0..=1.0` range —
/// extracted as its own pure function so the clamping (needed because
/// `done` can theoretically be reported as equal to `total` from a strided
/// call landing exactly on the last file, but never meaningfully exceeds
/// it) is unit-testable without constructing a `Gauge`/`Frame` at all, and
/// so a `total == 0` call (no files to scan — an edge case `SplashState`
/// itself does not prevent a caller from constructing) degrades to an empty
/// bar rather than a division-by-zero `NaN` reaching `Gauge::ratio`, which
/// panics on out-of-range input.
fn progress_ratio(done: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (done as f64 / total as f64).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rstest::rstest;

    #[rstest]
    #[case::should_return_zero_ratio_when_total_is_zero(0, 0, 0.0)]
    #[case::should_return_zero_ratio_when_done_is_zero(0, 10, 0.0)]
    #[case::should_return_half_ratio_when_done_is_half_of_total(5, 10, 0.5)]
    #[case::should_return_one_ratio_when_done_equals_total(10, 10, 1.0)]
    fn progress_ratio_cases(#[case] done: usize, #[case] total: usize, #[case] expected: f64) {
        let actual = progress_ratio(done, total);
        assert_eq!(expected, actual);
    }

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn should_render_logo_and_phase_label_when_progress_is_none() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        let state = SplashState::label_only("Resolving PR...");

        terminal
            .draw(|frame| draw_splash(frame, &state))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Resolving PR..."));
        // No "{done}/{total}" progress fraction anywhere in the buffer —
        // checked via the label the gauge itself would render
        // (`Gauge::label`'s own "{done}/{total}" format), not a bare `'/'`
        // scan: the logo's own ASCII art legitimately contains `/`/`\`
        // strokes, so a bare-slash check would false-positive on the logo
        // alone regardless of whether a gauge was drawn.
        assert!(!text.contains("0/0"));
    }

    #[test]
    fn should_render_progress_fraction_when_progress_is_some() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        let state = SplashState::with_progress("Building index...", 132, 842);

        terminal
            .draw(|frame| draw_splash(frame, &state))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Building index..."));
        assert!(text.contains("132/842"));
    }
}
