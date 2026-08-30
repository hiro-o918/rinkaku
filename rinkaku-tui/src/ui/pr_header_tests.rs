use super::*;
use crate::app::{App, BlastRadiusSelection};
use crate::locale::Locale;
use crate::review::PrContext;
use crate::stack::{PrAnalysisCache, StackEntry, StackPosition};
use crate::ui::draw;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{Report, ReportOrigin};
use rstest::rstest;
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

fn empty_report() -> Report {
    Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    }
}

fn pr_context() -> PrContext {
    PrContext {
        owner: "hiro-o918".to_string(),
        repo: "rinkaku".to_string(),
        number: 249,
        title: "add PR header tabs".to_string(),
        head_sha: "deadbeef".to_string(),
    }
}

fn stack_entries() -> Vec<StackEntry> {
    vec![
        StackEntry {
            number: 42,
            title: "auth".to_string(),
            head_ref_name: "auth".to_string(),
        },
        StackEntry {
            number: 43,
            title: "api".to_string(),
            head_ref_name: "api".to_string(),
        },
        StackEntry {
            number: 44,
            title: "frontend".to_string(),
            head_ref_name: "frontend".to_string(),
        },
    ]
}

fn tab(number: u64, title: &str, status: TabStatus) -> TabLabel {
    TabLabel {
        number,
        title: title.to_string(),
        status,
    }
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw_frame(app: &App, report: &Report, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| {
            draw(
                frame,
                app,
                report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");
    terminal
}

// --- header_content ---

#[test]
fn should_return_none_when_app_has_no_pr_context_or_stack() {
    let report = empty_report();
    let app = App::new(&report);

    let actual = header_content(&app);

    assert_eq!(None, actual);
}

#[test]
fn should_build_single_pr_content_from_pr_context_when_not_in_stack_mode() {
    let report = empty_report();
    let app = App::new(&report).with_pr_context(Some(pr_context()));

    let actual = header_content(&app);

    assert_eq!(
        Some(HeaderContent::Single {
            number: 249,
            title: "add PR header tabs".to_string(),
        }),
        actual
    );
}

#[test]
fn should_build_stack_content_with_ready_status_from_cache_slots() {
    let report = empty_report();
    let cache = Arc::new(PrAnalysisCache::new(3, 1));
    cache.set(
        0,
        crate::stack::Slot::Ready(Arc::new(crate::stack::PrAnalysis {
            report: empty_report(),
            diff_text: String::new(),
            pr: pr_context(),
        })),
    );
    cache.set(2, crate::stack::Slot::Failed("boom".to_string()));
    let app = App::new(&report)
        .with_stack(Some(StackPosition::new(stack_entries(), 1)))
        .with_pr_analysis_cache(Some(cache));

    let actual = header_content(&app);

    assert_eq!(
        Some(HeaderContent::Stack {
            tabs: vec![
                tab(42, "auth", TabStatus::Ready),
                tab(43, "api", TabStatus::Current),
                tab(44, "frontend", TabStatus::Failed),
            ],
            current: 1,
        }),
        actual
    );
}

#[test]
fn should_treat_missing_cache_slots_as_pending_when_cache_is_absent() {
    let report = empty_report();
    let app = App::new(&report).with_stack(Some(StackPosition::new(stack_entries(), 0)));

    let actual = header_content(&app);

    assert_eq!(
        Some(HeaderContent::Stack {
            tabs: vec![
                tab(42, "auth", TabStatus::Current),
                tab(43, "api", TabStatus::Pending),
                tab(44, "frontend", TabStatus::Pending),
            ],
            current: 0,
        }),
        actual
    );
}

// --- header_segments (pure layout) ---

#[test]
fn should_render_single_pr_title_and_hint_when_width_is_generous() {
    let content = HeaderContent::Single {
        number: 249,
        title: "add PR header tabs".to_string(),
    };

    let actual = header_segments(60, &content);

    assert_eq!(
        vec![
            segment("PR #249  add PR header tabs", SegmentKind::Title),
            segment(" ".repeat(23), SegmentKind::Separator),
            segment(HINT_TEXT, SegmentKind::Hint),
        ],
        actual
    );
}

#[test]
fn should_drop_the_hint_when_title_alone_fills_the_width() {
    let content = HeaderContent::Single {
        number: 249,
        title: "add PR header tabs".to_string(),
    };

    let actual = header_segments(10, &content);

    assert_eq!(
        vec![segment("PR #249  \u{2026}", SegmentKind::Title)],
        actual
    );
}

#[test]
fn should_render_every_tab_with_titles_when_width_is_generous() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(42, "auth", TabStatus::Ready),
            tab(43, "api", TabStatus::Current),
            tab(44, "frontend", TabStatus::Pending),
        ],
        current: 1,
    };

    let actual = header_segments(80, &content);

    assert_eq!(
        vec![
            segment("#42 auth", SegmentKind::OtherTab),
            segment(SEPARATOR, SegmentKind::Separator),
            segment("#43 api", SegmentKind::CurrentTab),
            segment(SEPARATOR, SegmentKind::Separator),
            segment("#44 frontend\u{2026}", SegmentKind::PendingTab),
            segment(
                " ".repeat(
                    80 - "#42 auth \u{2502} #43 api \u{2502} #44 frontend\u{2026}"
                        .chars()
                        .count()
                        - HINT_TEXT.chars().count()
                ),
                SegmentKind::Separator
            ),
            segment(HINT_TEXT, SegmentKind::Hint),
        ],
        actual
    );
}

#[test]
fn should_shrink_both_titles_to_keep_every_tab_visible_when_full_titles_overflow_width() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(1, &"x".repeat(30), TabStatus::Ready),
            tab(2, &"y".repeat(30), TabStatus::Current),
        ],
        current: 1,
    };

    let actual = header_segments(50, &content);

    assert_eq!(
        vec![
            segment(
                format!("#1 {}\u{2026}", "x".repeat(12)),
                SegmentKind::OtherTab
            ),
            segment(SEPARATOR, SegmentKind::Separator),
            segment(
                format!("#2 {}\u{2026}", "y".repeat(12)),
                SegmentKind::CurrentTab
            ),
            segment(" ".repeat(5), SegmentKind::Separator),
            segment(HINT_TEXT, SegmentKind::Hint),
        ],
        actual
    );
}

#[test]
fn should_reproduce_a_real_two_layer_stack_at_120_columns() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(251, &"a".repeat(70), TabStatus::Ready),
            tab(252, &"b".repeat(55), TabStatus::Current),
        ],
        current: 1,
    };

    let actual = header_segments(120, &content);

    assert_eq!(
        vec![
            segment(
                format!("#251 {}\u{2026}", "a".repeat(45)),
                SegmentKind::OtherTab
            ),
            segment(SEPARATOR, SegmentKind::Separator),
            segment(
                format!("#252 {}\u{2026}", "b".repeat(45)),
                SegmentKind::CurrentTab
            ),
            segment(" ".repeat(5), SegmentKind::Separator),
            segment(HINT_TEXT, SegmentKind::Hint),
        ],
        actual
    );
}

#[test]
fn should_keep_both_layers_visible_at_80_columns_when_cursor_is_on_the_second_layer() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(251, &"a".repeat(70), TabStatus::Ready),
            tab(252, &"b".repeat(55), TabStatus::Current),
        ],
        current: 1,
    };

    let actual = header_segments(80, &content);

    assert_eq!(
        vec![
            segment(
                format!("#251 {}\u{2026}", "a".repeat(25)),
                SegmentKind::OtherTab
            ),
            segment(SEPARATOR, SegmentKind::Separator),
            segment(
                format!("#252 {}\u{2026}", "b".repeat(25)),
                SegmentKind::CurrentTab
            ),
            segment(" ".repeat(5), SegmentKind::Separator),
            segment(HINT_TEXT, SegmentKind::Hint),
        ],
        actual
    );
}

#[test]
fn should_show_numbers_only_before_scrolling_when_shrinking_titles_falls_below_the_floor() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(100, "a-fairly-long-title-for-tab-one", TabStatus::Ready),
            tab(200, "a-fairly-long-title-for-tab-two", TabStatus::Current),
        ],
        current: 1,
    };

    let actual = header_segments(20, &content);

    assert_eq!(
        vec![
            segment("#100", SegmentKind::OtherTab),
            segment(SEPARATOR, SegmentKind::Separator),
            segment("#200", SegmentKind::CurrentTab),
        ],
        actual
    );
}

#[test]
fn should_scroll_off_other_tabs_only_once_even_a_numbers_only_strip_of_every_tab_does_not_fit() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(1, "one", TabStatus::Ready),
            tab(2, "two", TabStatus::Ready),
            tab(3, "three", TabStatus::Ready),
            tab(4, "four", TabStatus::Ready),
            tab(5, "five", TabStatus::Current),
        ],
        current: 4,
    };

    let actual = header_segments(6, &content);

    assert_eq!(vec![segment("#5", SegmentKind::CurrentTab)], actual);
}

#[test]
fn should_mark_a_failed_tab_with_a_trailing_bang() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(42, "auth", TabStatus::Current),
            tab(43, "api", TabStatus::Failed),
        ],
        current: 0,
    };

    let actual = header_segments(80, &content);

    assert!(
        actual
            .iter()
            .any(|seg| seg.text == "#43 api!" && seg.kind == SegmentKind::FailedTab)
    );
}

#[test]
fn should_drop_titles_before_dropping_tabs_when_width_is_short() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(42, "a-very-long-title-indeed", TabStatus::Ready),
            tab(43, "another-very-long-title", TabStatus::Current),
            tab(44, "yet-another-long-title", TabStatus::Ready),
        ],
        current: 1,
    };

    let actual = header_segments(30, &content);

    // NOTE: asserting only that every tab survived (numbers-only form)
    // rather than the exact byte layout — the point of this case is the
    // titles-before-tabs drop order, not the precise truncation math
    // `should_render_every_tab_with_titles_when_width_is_generous` already
    // pins.
    let rendered: String = actual.iter().map(|seg| seg.text.as_str()).collect();
    assert!(rendered.contains("#42"));
    assert!(rendered.contains("#43"));
    assert!(rendered.contains("#44"));
    assert!(!rendered.contains("very-long-title"));
}

#[test]
fn should_scroll_the_strip_to_keep_the_current_tab_in_view_when_the_stack_overflows_width() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(1, "one", TabStatus::Ready),
            tab(2, "two", TabStatus::Ready),
            tab(3, "three", TabStatus::Ready),
            tab(4, "four", TabStatus::Ready),
            tab(5, "five", TabStatus::Current),
        ],
        current: 4,
    };

    let actual = header_segments(20, &content);

    let rendered: String = actual.iter().map(|seg| seg.text.as_str()).collect();
    assert!(rendered.contains('5'));
    assert!(!rendered.contains('1'));
}

// --- header_segments (display width, CJK) ---

#[test]
fn should_fit_a_cjk_single_pr_title_exactly_when_width_equals_its_display_width() {
    let content = HeaderContent::Single {
        number: 249,
        title: "字".to_string(),
    };

    let actual = header_segments(11, &content);

    assert_eq!(vec![segment("PR #249  字", SegmentKind::Title)], actual);
}

#[test]
fn should_truncate_a_cjk_single_pr_title_on_a_column_boundary_when_one_column_short() {
    let content = HeaderContent::Single {
        number: 249,
        title: "字".to_string(),
    };

    let actual = header_segments(10, &content);

    assert_eq!(
        vec![segment("PR #249  \u{2026}", SegmentKind::Title)],
        actual
    );
}

#[rstest]
#[case::single(HeaderContent::Single { number: 249, title: "タイトル".to_string() })]
#[case::stack(HeaderContent::Stack {
    tabs: vec![tab(42, "認証機能", TabStatus::Current)],
    current: 0,
})]
fn should_keep_total_segment_width_within_budget_for_cjk_content(#[case] content: HeaderContent) {
    // NOTE: starts at width 3 (the numbers-only `#42` floor), not 0 — a
    // stack's current tab is always shown even narrower than that
    // (`tabs_fitting`'s documented "nothing narrower to fall back to"
    // exception), which `should_return_no_segments_for_stack_content_when_width_is_zero`
    // and `should_drop_a_cjk_stack_tab_title_when_one_column_short` already
    // cover on their own.
    for width in 3..=40 {
        let actual = header_segments(width, &content);

        let total_width: usize = actual.iter().map(|seg| seg.text.width()).sum();
        assert!(
            total_width <= width,
            "width {width}: segments {actual:?} summed to {total_width}"
        );
    }
}

#[test]
fn should_fit_a_cjk_stack_tab_title_exactly_when_width_equals_its_display_width() {
    let content = HeaderContent::Stack {
        tabs: vec![tab(42, "認証", TabStatus::Current)],
        current: 0,
    };

    let actual = header_segments(8, &content);

    assert_eq!(vec![segment("#42 認証", SegmentKind::CurrentTab)], actual);
}

#[test]
fn should_drop_a_cjk_stack_tab_title_when_one_column_short() {
    let content = HeaderContent::Stack {
        tabs: vec![tab(42, "認証", TabStatus::Current)],
        current: 0,
    };

    let actual = header_segments(7, &content);

    assert_eq!(vec![segment("#42", SegmentKind::CurrentTab)], actual);
}

#[test]
fn should_drop_a_wide_char_whole_when_it_would_straddle_the_truncation_cut() {
    // NOTE: width 10 leaves exactly 1 column after the ascii prefix, one
    // short of the 2-column 字 that would come next — chosen to land the
    // straddle exactly on that boundary.
    let content = HeaderContent::Single {
        number: 249,
        title: "字a".to_string(),
    };

    let actual = header_segments(10, &content);

    assert_eq!(
        vec![segment("PR #249  \u{2026}", SegmentKind::Title)],
        actual
    );
}

#[test]
fn should_return_no_segments_for_single_pr_content_when_width_is_zero() {
    let content = HeaderContent::Single {
        number: 249,
        title: "add PR header tabs".to_string(),
    };

    let actual = header_segments(0, &content);

    assert_eq!(Vec::<Segment>::new(), actual);
}

#[test]
fn should_return_no_segments_for_stack_content_when_width_is_zero() {
    let content = HeaderContent::Stack {
        tabs: vec![
            tab(42, "auth", TabStatus::Ready),
            tab(43, "api", TabStatus::Current),
        ],
        current: 1,
    };

    let actual = header_segments(0, &content);

    assert_eq!(Vec::<Segment>::new(), actual);
}

// --- draw_pr_header / draw (rendering) ---

#[test]
fn should_render_stack_header_row_above_the_entry_screen() {
    let report = empty_report();
    let app = App::new(&report)
        .with_stack(Some(StackPosition::new(stack_entries(), 1)))
        .with_pr_analysis_cache(None);

    let terminal = draw_frame(&app, &report, 80, 20);

    let text = buffer_text(&terminal);
    let header_row = text.lines().next().expect("header row");
    assert!(header_row.contains("#42 auth"));
    assert!(header_row.contains("#43 api"));
    assert!(header_row.contains("#44 frontend"));
    assert!(header_row.contains("open PR"));
}

#[test]
fn should_render_single_pr_header_row_above_the_entry_screen() {
    let report = empty_report();
    let app = App::new(&report).with_pr_context(Some(pr_context()));

    let terminal = draw_frame(&app, &report, 80, 20);

    let text = buffer_text(&terminal);
    let header_row = text.lines().next().expect("header row");
    assert!(header_row.contains("PR #249"));
    assert!(header_row.contains("add PR header tabs"));
}

#[test]
fn should_render_the_same_frame_as_before_the_header_existed_when_no_pr_context_is_set() {
    let report = empty_report();
    let app = App::new(&report);

    let terminal = draw_frame(&app, &report, 80, 20);

    let text = buffer_text(&terminal);
    // No `PrContext`/stack: the header row must not be reserved at all, so
    // the entry screen's own top border sits on row 0 — the exact layout a
    // pre-ADR-0076 frame would have drawn.
    let first_row = text.lines().next().expect("first row");
    assert!(
        first_row.contains("─"),
        "expected a pane border, got: {first_row}"
    );
}
