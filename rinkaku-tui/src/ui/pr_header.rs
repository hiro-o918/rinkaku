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

const SEPARATOR: &str = " \u{25b8} ";
const TRUNCATION_MARK: char = '\u{2026}';
/// Minimum display columns a tab's title keeps before
/// [`tabs_with_shrunk_titles`] gives up on titles altogether and falls back
/// to a numbers-only strip (ADR 0076 D2 amendment).
const MIN_TITLE_WIDTH: usize = 8;
/// Ceiling on a tab's title width even when the terminal is wide enough to
/// show more — keeps every layer's title similarly sized once a stack grows
/// past a couple of PRs (ADR 0076 D2 amendment) rather than the current tab
/// swallowing all spare width.
const MAX_TITLE_WIDTH: usize = 24;

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
    Stack {
        trunk: String,
        tabs: Vec<TabLabel>,
        current: usize,
    },
    Single {
        number: u64,
        title: String,
    },
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
            Some(HeaderContent::Stack {
                trunk: position.trunk().to_string(),
                tabs,
                current,
            })
        }
        None => app.pr_context().map(|ctx| HeaderContent::Single {
            number: ctx.number,
            title: ctx.title.clone(),
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    Label,
    Trunk,
    CurrentTab,
    OtherTab,
    PendingTab,
    FailedTab,
    Separator,
    Title,
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
/// truncates the title to at most `budget` display columns, further capped
/// at [`MAX_TITLE_WIDTH`] so a wide terminal doesn't stretch every title to
/// fill it.
fn tab_text(tab: &TabLabel, title_width: Option<usize>) -> (String, SegmentKind) {
    let suffix = match tab.status {
        TabStatus::Pending => "\u{2026}",
        TabStatus::Failed => "!",
        TabStatus::Current | TabStatus::Ready => "",
    };
    let text = match title_width {
        Some(budget) => {
            let title = truncate(&tab.title, budget.min(MAX_TITLE_WIDTH));
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

/// The leading `stack k/n` segment (`current`/`total` 1-based) — always
/// present regardless of width (ADR 0076 D2 amendment), so callers budget
/// remaining width around it rather than trying to drop it.
fn label_text(current: usize, total: usize) -> String {
    format!("stack {}/{}", current + 1, total)
}

/// Renders `trunk` + `tabs` into segments joined by [`SEPARATOR`], trying
/// every layer visible before dropping any (ADR 0076 D2 amendment), in
/// order: (1) trunk plus all tabs, full titles; (2) the same, titles shrunk
/// to share the available width; (3) the same, numbers only; (4) trunk
/// dropped, numbers only; (5) numbers-only tabs alone, scrolling the strip
/// so `current` stays in view. Mirrors [`super::scroll`]'s "requested
/// value, caller clamps to what's visible" split: this function decides
/// *what* fits, not how a real terminal wraps it.
fn stack_segments(width: usize, trunk: &str, tabs: &[TabLabel], current: usize) -> Vec<Segment> {
    all_tabs_visible(width, trunk, tabs).unwrap_or_else(|| tabs_fitting(width, tabs, current))
}

/// Steps (1)-(4) of [`stack_segments`]: every layer's tab, at full, shrunk,
/// or numbers-only titles, with the trunk name shown whenever it fits and
/// dropped as a last resort before scrolling. `None` once even a
/// numbers-only strip of every tab (no trunk) doesn't fit `width` — the
/// boundary [`header_segments`] uses to fall back to scrolling.
fn all_tabs_visible(width: usize, trunk: &str, tabs: &[TabLabel]) -> Option<Vec<Segment>> {
    if tabs.is_empty() {
        return Some(Vec::new());
    }
    let last = tabs.len() - 1;
    let full_titles = render_tab_window(tabs, 0..=last, Some(usize::MAX));
    if let Some(segments) = with_trunk(width, trunk, full_titles) {
        return Some(segments);
    }
    if let Some(shrunk) = tabs_with_shrunk_titles(width, trunk, tabs) {
        return Some(shrunk);
    }
    let numbers_only = render_tab_window(tabs, 0..=last, None);
    if let Some(segments) = with_trunk(width, trunk, numbers_only.clone()) {
        return Some(segments);
    }
    (strip_width(&numbers_only) <= width).then_some(numbers_only)
}

/// Prepends `trunk` and its arrow to `tabs` when the combination fits
/// `width`, `None` otherwise (ADR 0076 D2's "drop the trunk name before
/// scrolling" rule).
fn with_trunk(width: usize, trunk: &str, tabs: Vec<Segment>) -> Option<Vec<Segment>> {
    let mut segments = vec![
        segment(trunk.to_string(), SegmentKind::Trunk),
        segment(SEPARATOR, SegmentKind::Separator),
    ];
    segments.extend(tabs);
    (strip_width(&segments) <= width).then_some(segments)
}

/// All tabs with titles truncated to an equal per-tab column budget
/// (rather than proportionally to title length, so every tab ends up
/// equally readable), with the trunk name shown when it still fits.
/// `None` if `width` can't fit every tab even at [`MIN_TITLE_WIDTH`].
fn tabs_with_shrunk_titles(width: usize, trunk: &str, tabs: &[TabLabel]) -> Option<Vec<Segment>> {
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
    let shrunk = render_tab_window(tabs, 0..=last, Some(per_tab_budget));
    Some(with_trunk(width, trunk, shrunk.clone()).unwrap_or(shrunk))
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
/// (ADR 0076 D3). Single-PR mode is just a truncated title line. Stack mode
/// prepends the `stack k/n` label (always shown, ADR 0076 D2 amendment) and
/// budgets the rest of `width` for the trunk name and tabs via
/// [`stack_segments`].
pub(crate) fn header_segments(width: usize, content: &HeaderContent) -> Vec<Segment> {
    if width == 0 {
        return Vec::new();
    }
    match content {
        HeaderContent::Stack {
            trunk,
            tabs,
            current,
        } => {
            let label = label_text(*current, tabs.len());
            if label.width() >= width {
                return vec![segment(truncate(&label, width), SegmentKind::Label)];
            }
            let label_reserve = label.width() + 2;
            let mut segments = vec![segment(label, SegmentKind::Label)];
            let remaining = width.saturating_sub(label_reserve);
            if remaining == 0 {
                return segments;
            }
            segments.push(segment(" ".repeat(2), SegmentKind::Separator));
            segments.extend(stack_segments(remaining, trunk, tabs, *current));
            segments
        }
        HeaderContent::Single { number, title } => {
            let full = format!("PR #{number}  {title}");
            vec![segment(truncate(&full, width), SegmentKind::Title)]
        }
    }
}

fn style_for(kind: SegmentKind) -> Style {
    match kind {
        SegmentKind::CurrentTab => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        SegmentKind::OtherTab | SegmentKind::Title | SegmentKind::Trunk => Style::default(),
        SegmentKind::PendingTab => Style::default().fg(Color::DarkGray),
        SegmentKind::FailedTab => Style::default().fg(Color::Red),
        SegmentKind::Separator | SegmentKind::Label => Style::default().fg(Color::DarkGray),
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
