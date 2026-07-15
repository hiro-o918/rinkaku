//! Tests for `crate::ui::scroll`, split from the source file (ADR 0028)
//! and grouped by which pure helper each block pins:
//!
//! - `clamp_and_indicator` — `clamp_scroll`/`scroll_indicator`
//! - `windowing` — `visible_index_window`/`window_overflow_indicators`/
//!   `windowed_rows_with_indicators` (the #61-review cursor-follow fix)
//! - `wrap_lines` — `wrap_lines_with_origins`/`wrap_one_line`
//! - `truncation` — `truncate_to_width`/`truncate_to_width_keeping_tail`/
//!   `truncate_line_to_width`
//! - `pair_wrap` — ADR 0044's per-side wrap-then-pad helper for the Diff
//!   pane's split view
//! - `wrap_origins` — the logical-line <-> display-row conversion
//!   (`logical_line_to_display_row`/`display_row_to_logical_line`) this
//!   scroll-unit fix adds

use super::*;

mod clamp_and_indicator;
mod pair_wrap;
mod truncation;
mod windowing;
mod wrap_lines;
mod wrap_origins;
