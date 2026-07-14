//! Tests for `crate::ui::scroll`, split from the source file (ADR 0028)
//! and grouped by which pure helper each block pins:
//!
//! - `clamp_and_indicator` — `clamp_scroll`/`scroll_indicator`
//! - `windowing` — `visible_index_window`/`window_overflow_indicators`/
//!   `windowed_rows_with_indicators` (the #61-review cursor-follow fix)
//! - `wrap_lines` — `wrap_lines`/`wrap_one_line`
//! - `truncation` — `truncate_to_width`/`truncate_to_width_keeping_tail`/
//!   `truncate_line_to_width`

use super::*;

mod clamp_and_indicator;
mod truncation;
mod windowing;
mod wrap_lines;
