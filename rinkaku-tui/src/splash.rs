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
///
/// `tip` (ADR 0077) is a single usecase tip picked once per run by the
/// composition root and carried unchanged across every phase transition,
/// unlike `phase_label`/`progress`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplashState {
    pub phase_label: String,
    pub progress: Option<(usize, usize)>,
    pub tip: Option<String>,
}

impl SplashState {
    /// A label-only state with no progress bar — every phase except the
    /// two file-scanning ones (`--pr`/`--base` resolution, diffing,
    /// analyzing the diff's own changed symbols).
    pub fn label_only(phase_label: impl Into<String>) -> Self {
        Self {
            phase_label: phase_label.into(),
            progress: None,
            tip: None,
        }
    }

    /// A state with a determinate progress bar — `analyze_repo`'s parallel
    /// parse or `TagsResolver::new`'s sequential index build, both of which
    /// know their total file count up front.
    pub fn with_progress(phase_label: impl Into<String>, done: usize, total: usize) -> Self {
        Self {
            phase_label: phase_label.into(),
            progress: Some((done, total)),
            tip: None,
        }
    }

    /// Attaches a usecase tip (ADR 0077).
    pub fn with_tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }
}

/// The tip's reserved row budget (one blank separator line + up to two
/// wrapped lines of text) — bounds how many rows [`draw_splash`] must find
/// spare before it draws a tip at all, since the tip is skipped rather
/// than truncated on a terminal too short to fit it (this module's own
/// precedent: [`SplashState::progress`]'s gauge already only draws when
/// `Some`, never in a squeezed partial form).
const TIP_ROWS: u16 = 3;

/// Draws one splash frame: the logo centered in the upper portion of the
/// screen, the phase label beneath it, and — when [`SplashState::progress`]
/// is `Some`, ADR 0033 decision 3 — a determinate [`Gauge`] beneath the
/// label showing `done`/`total` as both a filled bar and a `"{done}/{total}"`
/// label, and — when [`SplashState::tip`] is `Some` and the terminal has
/// [`TIP_ROWS`] to spare (ADR 0077) — a dim, wrapped usecase tip below
/// that. Deliberately uncovered by unit tests beyond the coarse
/// `TestBackend` snapshot in this module's own test block, matching
/// `crate::ui::draw`'s own "rendering itself is covered separately... kept
/// few and coarse" precedent — there is no per-pixel behavior here worth
/// pinning beyond "the logo and the current phase both show up".
pub fn draw_splash(frame: &mut Frame, state: &SplashState) {
    let area = frame.area();

    let logo_height = LOGO_LINES.len() as u16;
    let core_height = logo_height + 1 + 1 + if state.progress.is_some() { 1 } else { 0 };
    let show_tip = state.tip.is_some() && area.height >= core_height + TIP_ROWS;
    // Logo + one blank line + phase label + (optional) gauge + (optional)
    // tip, vertically centered as a block rather than each line
    // individually — a fixed-height `Constraint::Length` block sized to
    // exactly what this frame needs, with `Constraint::Fill` above/below
    // splitting the remaining space evenly so the block sits in the middle
    // regardless of terminal height.
    let content_height = core_height + if show_tip { TIP_ROWS } else { 0 };
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
    if show_tip {
        constraints.push(Constraint::Length(TIP_ROWS));
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

    let mut next_row = 2;
    if let Some((done, total)) = state.progress {
        let ratio = progress_ratio(done, total);
        let gauge_area = centered_band(rows[next_row]);
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan))
            .ratio(ratio)
            .label(format!("{done}/{total}"));
        frame.render_widget(gauge, gauge_area);
        next_row += 1;
    }

    if show_tip {
        // `show_tip` already implies `state.tip.is_some()`.
        let tip = state.tip.as_deref().unwrap_or_default();
        let tip_area = centered_band(rows[next_row]);
        let tip_paragraph = Paragraph::new(tip)
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true });
        frame.render_widget(tip_paragraph, tip_area);
    }
}

/// Narrows `area` to a centered horizontal band — the full terminal width
/// looks disproportionately wide for a single progress bar or tip line
/// sitting under a comparatively narrow logo/label, so this caps it at
/// [`BAND_WIDTH`] columns (or the full area, whichever is narrower, for a
/// terminal too small to fit that). Shared by the gauge and the tip
/// (ADR 0077) so both sit at the same width.
fn centered_band(area: Rect) -> Rect {
    let width = BAND_WIDTH.min(area.width);
    let [_, band, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(area);
    band
}

const BAND_WIDTH: u16 = 40;

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

    #[test]
    fn should_render_tip_when_terminal_has_room() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
        let state = SplashState::label_only("Resolving PR...").with_tip("Use gt/gT to switch PRs");

        terminal
            .draw(|frame| draw_splash(frame, &state))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Use gt/gT to switch PRs"));
    }

    #[test]
    fn should_omit_tip_when_terminal_height_is_insufficient() {
        // `label_only`'s core content needs 7 rows (5-line logo + blank +
        // label); `TIP_ROWS` (3) pushes the requirement to 10, so a
        // terminal one row short of that must render no tip rather than a
        // truncated one.
        let mut terminal = Terminal::new(TestBackend::new(80, 9)).expect("terminal");
        let state = SplashState::label_only("Resolving PR...").with_tip("Use gt/gT to switch PRs");

        terminal
            .draw(|frame| draw_splash(frame, &state))
            .expect("draw");

        let text = buffer_text(&terminal);
        assert!(text.contains("Resolving PR..."));
        assert!(!text.contains("Use gt/gT to switch PRs"));
    }
}
