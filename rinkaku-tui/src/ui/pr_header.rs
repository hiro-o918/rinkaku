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
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const HINT_TEXT: &str = "w: open PR";
const SEPARATOR: &str = " \u{2502} ";
const TRUNCATION_MARK: char = '\u{2026}';
/// Minimum display columns a tab's title keeps before
/// [`tabs_with_shrunk_titles`] gives up on titles altogether and falls back
/// to a numbers-only strip (ADR 0076 D2 amendment).
const MIN_TITLE_WIDTH: usize = 8;

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
/// whole strip fit" truncation pass below. `title_width` is `None` for a
/// numbers-only tab (dropped last, ADR 0076 D2 amendment); `Some(budget)`
/// truncates the title to at most `budget` display columns.
fn tab_text(tab: &TabLabel, title_width: Option<usize>) -> (String, SegmentKind) {
    let suffix = match tab.status {
        TabStatus::Pending => "\u{2026}",
        TabStatus::Failed => "!",
        TabStatus::Current | TabStatus::Ready => "",
    };
    let text = match title_width {
        Some(budget) => {
            let title = truncate(&tab.title, budget);
            let suffix = if title.ends_with(suffix) { "" } else { suffix };
            format!("#{} {title}{suffix}", tab.number)
        }
        None => format!("#{}{suffix}", tab.number),
    };
    let kind = match tab.status {
        TabStatus::Current => SegmentKind::CurrentTab,
        TabStatus::Ready => SegmentKind::OtherTab,
        TabStatus::Pending => SegmentKind::PendingTab,
        TabStatus::Failed => SegmentKind::FailedTab,
    };
    (text, kind)
}

/// Renders `tabs` into tab segments joined by [`SEPARATOR`], trying every
/// layer visible before dropping any (ADR 0076 D2 amendment), in order:
/// (1) all tabs, full titles; (2) all tabs, titles shrunk to share the
/// available width; (3) all tabs, numbers only; (4) numbers-only, scrolling
/// the strip so `current` stays in view. Mirrors [`super::scroll`]'s
/// "requested value, caller clamps to what's visible" split: this function
/// decides *what* fits, not how a real terminal wraps it.
fn stack_segments(width: usize, tabs: &[TabLabel], current: usize) -> Vec<Segment> {
    all_tabs_visible(width, tabs).unwrap_or_else(|| tabs_fitting(width, tabs, current))
}

/// Steps (1)-(3) of [`stack_segments`]: every layer's tab, at full,
/// shrunk, or numbers-only titles. `None` once even numbers-only doesn't
/// fit `width` — the boundary [`header_segments`] uses to keep dropping
/// the hint a lower-priority resort than dropping/scrolling tabs.
fn all_tabs_visible(width: usize, tabs: &[TabLabel]) -> Option<Vec<Segment>> {
    if tabs.is_empty() {
        return Some(Vec::new());
    }
    let last = tabs.len() - 1;
    let full_titles = render_tab_window(tabs, 0..=last, Some(usize::MAX));
    if strip_width(&full_titles) <= width {
        return Some(full_titles);
    }
    if let Some(shrunk) = tabs_with_shrunk_titles(width, tabs) {
        return Some(shrunk);
    }
    let numbers_only = render_tab_window(tabs, 0..=last, None);
    (strip_width(&numbers_only) <= width).then_some(numbers_only)
}

/// All tabs with titles truncated to an equal per-tab column budget
/// (rather than proportionally to title length, so every tab ends up
/// equally readable), `None` if `width` can't fit every tab even at
/// [`MIN_TITLE_WIDTH`].
fn tabs_with_shrunk_titles(width: usize, tabs: &[TabLabel]) -> Option<Vec<Segment>> {
    let last = tabs.len() - 1;
    let fixed_width = strip_width(&render_tab_window(tabs, 0..=last, None));
    let separators_width = SEPARATOR.width() * tabs.len().saturating_sub(1);
    let space_before_each_title = tabs.len();
    let available_for_titles = width
        .saturating_sub(fixed_width)
        .saturating_sub(separators_width)
        .saturating_sub(space_before_each_title);
    let per_tab_budget = available_for_titles / tabs.len();
    if per_tab_budget < MIN_TITLE_WIDTH {
        return None;
    }
    Some(render_tab_window(tabs, 0..=last, Some(per_tab_budget)))
}

/// The widest contiguous window of numbers-only `tabs` that includes
/// `current`, grown one tab at a time (alternating toward the top of the
/// stack first, then the bottom) while the strip still fits `width` — the
/// scrolling behaviour ADR 0076 D2 asks for once even a numbers-only strip
/// of every tab doesn't fit. Always includes `current` even when its own
/// tab alone overflows `width` (the caller has nothing narrower to fall
/// back to at that point).
fn tabs_fitting(width: usize, tabs: &[TabLabel], current: usize) -> Vec<Segment> {
    let mut start = current;
    let mut end = current;
    loop {
        let up = (end + 1 < tabs.len()).then(|| (start, end + 1));
        let down = (start > 0).then(|| (start - 1, end));
        let grown = [up, down]
            .into_iter()
            .flatten()
            .find(|&(s, e)| strip_width(&render_tab_window(tabs, s..=e, None)) <= width);
        let Some((next_start, next_end)) = grown else {
            break;
        };
        start = next_start;
        end = next_end;
    }
    render_tab_window(tabs, start..=end, None)
}

fn render_tab_window(
    tabs: &[TabLabel],
    range: std::ops::RangeInclusive<usize>,
    title_width: Option<usize>,
) -> Vec<Segment> {
    let mut segments = Vec::new();
    for (offset, index) in range.clone().enumerate() {
        if offset > 0 {
            segments.push(segment(SEPARATOR, SegmentKind::Separator));
        }
        let (text, kind) = tab_text(&tabs[index], title_width);
        segments.push(segment(text, kind));
    }
    segments
}

fn strip_width(segments: &[Segment]) -> usize {
    segments.iter().map(|s| s.text.width()).sum()
}

/// Truncates `text` to at most `max` display columns, replacing the tail
/// with [`TRUNCATION_MARK`] (1 column) when it doesn't fit whole — measured
/// with [`UnicodeWidthChar::width`] (`unwrap_or(1)` fallback, matching
/// `super::scroll`'s convention) rather than `char` count, so a wide (e.g.
/// CJK) character is dropped whole rather than sliced in half. `max == 0`
/// returns an empty string rather than panicking on the zero-width slice.
fn truncate(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let budget = max - 1;
    let mut truncated = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(1);
        if used + char_width > budget {
            break;
        }
        truncated.push(ch);
        used += char_width;
    }
    truncated.push(TRUNCATION_MARK);
    truncated
}

/// Builds the header row's segments for a frame `width` columns wide —
/// takes no `ratatui`/`App` types so it is unit-testable in isolation
/// (ADR 0076 D3). Tries, in order: every tab visible (full, then shrunk,
/// then numbers-only titles) with the `w: open PR` hint; the same without
/// the hint; only then (stack mode) scrolling the numbers-only strip, or
/// (single-PR mode) truncating the title further. The hint is dropped
/// before tabs are ever dropped/scrolled — [`body_with_every_tab_visible`]
/// is what keeps that order, since it is `None` exactly when scrolling
/// would otherwise be needed.
pub(crate) fn header_segments(width: usize, content: &HeaderContent) -> Vec<Segment> {
    let hint_reserve = HINT_TEXT.width() + 2;
    let hint_room_width = width.saturating_sub(hint_reserve).max(1);
    if let Some(segments) = body_with_every_tab_visible(hint_room_width, content)
        .and_then(|body| with_hint(width, body))
    {
        return segments;
    }
    if let Some(body) = body_with_every_tab_visible(width, content) {
        return body;
    }
    body_segments(width, content)
}

/// `body_segments`, but `None` for stack mode once the strip would need to
/// scroll (single-PR mode never scrolls, so it always returns `Some`) —
/// see [`header_segments`] for why that distinction matters.
fn body_with_every_tab_visible(width: usize, content: &HeaderContent) -> Option<Vec<Segment>> {
    if width == 0 {
        return Some(Vec::new());
    }
    match content {
        HeaderContent::Stack { tabs, .. } => all_tabs_visible(width, tabs),
        HeaderContent::Single { .. } => Some(body_segments(width, content)),
    }
}

fn body_segments(width: usize, content: &HeaderContent) -> Vec<Segment> {
    if width == 0 {
        return Vec::new();
    }
    match content {
        HeaderContent::Stack { tabs, current } => stack_segments(width, tabs, *current),
        HeaderContent::Single { number, title } => {
            let full = format!("PR #{number}  {title}");
            vec![segment(truncate(&full, width), SegmentKind::Title)]
        }
    }
}

/// Appends [`HINT_TEXT`] right-aligned when `body` plus the hint (and a
/// minimum two-space gap) fits `width`, `None` otherwise (ADR 0076 D2's
/// "drop the hint first" rule) — the caller then falls back to
/// [`body_segments`] computed against the *full* `width`, so a body that
/// didn't need to shrink for the hint isn't shrunk needlessly.
fn with_hint(width: usize, body: Vec<Segment>) -> Option<Vec<Segment>> {
    let hint_len = HINT_TEXT.width();
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
