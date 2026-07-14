//! Tests for `crate::split_pairing`, split from the source file (ADR 0028).
//! One test module today (`pair_hunk_lines`, `crate::diff_shape::pair_hunk_lines`'s
//! own re-exported entry point) — kept as a `mod.rs` rather than a single
//! flat test file so a future second pub function here follows the same
//! per-function grouping `diff_shape_tests/mod.rs` already established.

use super::*;

mod pair_hunk_lines;
