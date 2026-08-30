//! PR header row (ADR 0076): a one-line tab strip in stack mode, or a plain
//! title line in single-PR mode, drawn above the entry/source screens
//! whenever the session has a [`crate::review::PrContext`]. The text/
//! truncation layout is a pure function ([`header_segments`]) so its shape
//! is unit-testable without a live `Frame`; [`draw_pr_header`] only turns
//! its output into styled spans.

use crate::app::App;
use crate::stack::Slot;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const HINT_TEXT: &str = "w: open PR";
const SEPARATOR: &str = " \u{2502} ";
const TRUNCATION_MARK: char = '\u{2026}';

/// One tab's live status, resolved from [`crate::stack::PrAnalysisCache`] at
/// draw time (ADR 0076 D3) — the cursor's own layer is always [`Self::Ready`]
/// (it is on screen), so [`Slot::InProgress`]/[`Slot::Pending`] and
/// [`Slot::Ready`] both collapse to a status describing *other* layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabStatus {
    Current,
    Ready,
    Pending,
    Failed,
}

impl TabStatus {
    fn from_slot(slot: &Slot, is_current: bool) -> Self {
        if is_current {
            return Self::Current;
        }
        match slot {
            Slot::Ready(_) => Self::Ready,
            Slot::Pending | Slot::InProgress => Self::Pending,
            Slot::Failed(_) => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabLabel {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) status: TabStatus,
}

/// What the header has to render, resolved from `App` before
/// [`header_segments`] runs — kept separate from `App` itself so the pure
/// layout function takes no `ratatui`/`App` types at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HeaderContent {
    Stack { tabs: Vec<TabLabel>, current: usize },
    Single { number: u64, title: String },
}

/// Builds [`HeaderContent`] from `app`'s current `PrContext`/stack/cache,
/// `None` outside `--pr` mode (`ui::draw`'s own gate on whether to reserve
/// the header's row at all).
pub(crate) fn header_content(app: &App) -> Option<HeaderContent> {
    match app.stack() {
        Some(position) => {
            let cache = app.pr_analysis_cache();
            let current = position.cursor();
            let tabs = position
                .entries()
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let slot = cache.map(|cache| cache.get(index)).unwrap_or(Slot::Pending);
                    TabLabel {
                        number: entry.number,
                        title: entry.title.clone(),
                        status: TabStatus::from_slot(&slot, index == current),
                    }
                })
                .collect();
            Some(HeaderContent::Stack { tabs, current })
        }
        None => app.pr_context().map(|ctx| HeaderContent::Single {
            number: ctx.number,
            title: ctx.title.clone(),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    CurrentTab,
    OtherTab,
    PendingTab,
    FailedTab,
    Separator,
    Title,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    pub(crate) text: String,
    pub(crate) kind: SegmentKind,
}

fn segment(text: impl Into<String>, kind: SegmentKind) -> Segment {
    Segment {
        text: text.into(),
        kind,
    }
}

/// One tab's own rendered text and [`SegmentKind`], before the "does the
/// whole strip fit" truncation pass below — `with_title` controls whether
/// the title is included at all (dropped first when width is short, ADR
/// 0076 D2).
fn tab_text(tab: &TabLabel, with_title: bool) -> (String, SegmentKind) {
    let suffix = match tab.status {
        TabStatus::Pending => "\u{2026}",
        TabStatus::Failed => "!",
        TabStatus::Current | TabStatus::Ready => "",
    };
    let text = if with_title {
        format!("#{} {}{suffix}", tab.number, tab.title)
    } else {
        format!("#{}{suffix}", tab.number)
    };
    let kind = match tab.status {
        TabStatus::Current => SegmentKind::CurrentTab,
        TabStatus::Ready => SegmentKind::OtherTab,
        TabStatus::Pending => SegmentKind::PendingTab,
        TabStatus::Failed => SegmentKind::FailedTab,
    };
    (text, kind)
}

/// Renders `tabs` into tab segments joined by [`SEPARATOR`], scrolling the
/// strip (dropping tabs off one end) so `current`'s own tab is always
/// included, then dropping titles (numbers only) if the full-title strip
/// still doesn't fit `width`. Mirrors [`super::scroll`]'s "requested value,
/// caller clamps to what's visible" split: this function decides *what*
/// fits, not how a real terminal wraps it.
fn stack_segments(width: usize, tabs: &[TabLabel], current: usize) -> Vec<Segment> {
    let with_titles = tabs_fitting(width, tabs, current, true);
    if strip_width(&with_titles) <= width {
        return with_titles;
    }
    tabs_fitting(width, tabs, current, false)
}

/// The widest contiguous window of `tabs` that includes `current`, rendered
/// with or without titles, grown one tab at a time (alternating toward the
/// top of the stack first, then the bottom) while the strip still fits
/// `width` — the scrolling behaviour ADR 0076 D2 asks for. Always includes
/// `current` even when its own tab alone overflows `width` (the caller has
/// nothing narrower to fall back to at that point).
fn tabs_fitting(width: usize, tabs: &[TabLabel], current: usize, with_title: bool) -> Vec<Segment> {
    let mut start = current;
    let mut end = current;
    loop {
        let up = (end + 1 < tabs.len()).then(|| (start, end + 1));
        let down = (start > 0).then(|| (start - 1, end));
        let grown = [up, down]
            .into_iter()
            .flatten()
            .find(|&(s, e)| strip_width(&render_tab_window(tabs, s..=e, with_title)) <= width);
        let Some((next_start, next_end)) = grown else {
            break;
        };
        start = next_start;
        end = next_end;
    }
    render_tab_window(tabs, start..=end, with_title)
}

fn render_tab_window(
    tabs: &[TabLabel],
    range: std::ops::RangeInclusive<usize>,
    with_title: bool,
) -> Vec<Segment> {
    let mut segments = Vec::new();
    for (offset, index) in range.clone().enumerate() {
        if offset > 0 {
            segments.push(segment(SEPARATOR, SegmentKind::Separator));
        }
        let (text, kind) = tab_text(&tabs[index], with_title);
        segments.push(segment(text, kind));
    }
    segments
}

fn strip_width(segments: &[Segment]) -> usize {
    segments.iter().map(|s| s.text.chars().count()).sum()
}

/// Truncates `text` to at most `max` columns, replacing the last visible
/// character with [`TRUNCATION_MARK`] when it doesn't fit whole — `max == 0`
/// returns an empty string rather than panicking on the zero-width slice.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut truncated: String = text.chars().take(max - 1).collect();
    truncated.push(TRUNCATION_MARK);
    truncated
}

/// Builds the header row's segments for a frame `width` columns wide —
/// takes no `ratatui`/`App` types so it is unit-testable in isolation
/// (ADR 0076 D3). Tries, in order: full content with the `w: open PR` hint,
/// then without the hint, then (single-PR mode: truncating the title;
/// stack mode: narrowing/dropping tab titles) until the strip fits.
pub(crate) fn header_segments(width: usize, content: &HeaderContent) -> Vec<Segment> {
    let hint_reserve = HINT_TEXT.chars().count() + 2;
    let body_with_hint_room = body_segments(width.saturating_sub(hint_reserve).max(1), content);
    if let Some(segments) = with_hint(width, body_with_hint_room) {
        return segments;
    }
    body_segments(width, content)
}

fn body_segments(width: usize, content: &HeaderContent) -> Vec<Segment> {
    match content {
        HeaderContent::Stack { tabs, current } => stack_segments(width, tabs, *current),
        HeaderContent::Single { number, title } => {
            let full = format!("PR #{number}  {title}");
            vec![segment(truncate(&full, width.max(1)), SegmentKind::Title)]
        }
    }
}

/// Appends [`HINT_TEXT`] right-aligned when `body` plus the hint (and a
/// minimum two-space gap) fits `width`, `None` otherwise (ADR 0076 D2's
/// "drop the hint first" rule) — the caller then falls back to
/// [`body_segments`] computed against the *full* `width`, so a body that
/// didn't need to shrink for the hint isn't shrunk needlessly.
fn with_hint(width: usize, body: Vec<Segment>) -> Option<Vec<Segment>> {
    let hint_len = HINT_TEXT.chars().count();
    let body_len = strip_width(&body);
    if body_len + 2 + hint_len > width {
        return None;
    }
    let padding = width - body_len - hint_len;
    let mut segments = body;
    segments.push(segment(" ".repeat(padding), SegmentKind::Separator));
    segments.push(segment(HINT_TEXT, SegmentKind::Hint));
    Some(segments)
}

fn style_for(kind: SegmentKind) -> Style {
    match kind {
        SegmentKind::CurrentTab => {
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
        }
        SegmentKind::OtherTab | SegmentKind::Title => Style::default(),
        SegmentKind::PendingTab => Style::default().fg(Color::DarkGray),
        SegmentKind::FailedTab => Style::default().fg(Color::Red),
        SegmentKind::Separator => Style::default().fg(Color::DarkGray),
        SegmentKind::Hint => Style::default().fg(Color::DarkGray),
    }
}

/// Draws the header row (ADR 0076) into `area` — a single line, no borders,
/// matching the status line's own bare-`Paragraph` styling
/// (`super::status::draw_status_line`). A no-op caller-side gate
/// ([`header_content`] returning `None`) keeps this from drawing at all
/// outside `--pr` mode; `ui::draw` only calls this once it already has
/// `Some(content)`.
pub(crate) fn draw_pr_header(frame: &mut Frame, content: &HeaderContent, area: Rect) {
    let segments = header_segments(area.width as usize, content);
    let spans: Vec<Span<'static>> = segments
        .into_iter()
        .map(|seg| Span::styled(seg.text, style_for(seg.kind)))
        .collect();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
#[path = "pr_header_tests.rs"]
mod tests;
