//! Optional progress reporting for the pipeline's two whole-repository
//! file-scanning phases (ADR 0033): [`crate::pipeline::analyze_repo`]'s
//! rayon-parallel parse and [`crate::deps::TagsResolver::new`]'s sequential
//! indexing pass.
//!
//! Defined here (the consumer side, per CLAUDE.md's "ports as traits,
//! defined on the consumer side" rule) as a plain callback type rather than
//! a trait, matching the existing `read_file`-shaped ports elsewhere in this
//! crate (`pipeline::analyze_diff`'s `read_file`/`read_base_file`): a single
//! `(done, total)` call is all either phase needs, so a one-method trait
//! would add a name (`ProgressSink`, `Reporter`, ...) without adding
//! anything a closure type doesn't already say more directly.
//!
//! `Sync` (not `Send`) is the bound both call sites need: `analyze_repo`'s
//! rayon workers call the same `&(dyn Fn(usize, usize) + Sync)` reference
//! concurrently from multiple threads without moving it, so `Sync` (safe to
//! share a `&reference` across threads) is what's required — `Send` (safe
//! to move a value to another thread) is not, since the callback itself
//! never crosses a thread boundary, only calls through a shared reference
//! do. `TagsResolver::new`'s sequential loop does not need the bound at all,
//! but takes the same type for call-site symmetry (`main.rs` builds one
//! closure and passes `Some(&closure)` to both).
//!
//! `main.rs` is the only real caller (`--tui` mode, ADR 0033); every other
//! caller — every other display mode, every existing test — passes `None`,
//! which both `analyze_repo` and `TagsResolver::new` treat as "do not call
//! this at all", leaving their behavior and output byte-for-byte unchanged
//! from before this parameter existed.
pub type OnProgress<'a> = &'a (dyn Fn(usize, usize) + Sync);

/// How many completed files must pass between two [`OnProgress`] calls for
/// the same phase — both [`crate::pipeline::analyze_repo`]'s parallel loop
/// and [`crate::deps::TagsResolver::new`]'s sequential one use this same
/// constant so a redraw at "file 16 of 842" means the same thing regardless
/// of which phase produced it.
///
/// Chosen as a fixed stride rather than a time-based throttle (e.g. "redraw
/// at most every N milliseconds"): a fixed file-count stride needs no clock
/// read on the hot per-file path (`std::time::Instant::now()` inside a
/// rayon worker's per-file closure would itself cost something on every
/// single file, working against the very thing this stride exists to
/// avoid), and produces a bounded, predictable number of redraws for a
/// given repository size (`total / 16`, roughly) rather than one that
/// depends on how fast each file happens to parse.
pub const PROGRESS_REPORT_STRIDE: usize = 16;

/// Whether the file at `completed_count` (1-indexed: the count *after* this
/// file finished, matching how both call sites increment their counter
/// before checking) should trigger an [`OnProgress`] call — every
/// [`PROGRESS_REPORT_STRIDE`]th file, plus always the very last one so a
/// caller watching the callback sees a final `(total, total)` call and
/// knows the phase actually reached completion, rather than potentially
/// stopping short at the last stride boundary before `total`.
///
/// Pure and total, extracted out of both call sites so this exact rule is
/// unit-testable without a real rayon pool or a large file list.
pub fn should_report_progress(completed_count: usize, total: usize) -> bool {
    if total == 0 {
        return false;
    }
    completed_count == total || completed_count.is_multiple_of(PROGRESS_REPORT_STRIDE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_report_at_first_stride_boundary(16, 100, true)]
    #[case::should_not_report_between_stride_boundaries(15, 100, false)]
    #[case::should_not_report_between_stride_boundaries_just_past_one(17, 100, false)]
    #[case::should_report_at_second_stride_boundary(32, 100, true)]
    #[case::should_report_on_final_file_even_off_stride(97, 97, true)]
    #[case::should_report_on_final_file_when_total_is_smaller_than_stride(5, 5, true)]
    #[case::should_not_report_on_non_final_file_smaller_than_stride(3, 5, false)]
    #[case::should_not_report_when_total_is_zero(0, 0, false)]
    fn should_report_progress_cases(
        #[case] completed_count: usize,
        #[case] total: usize,
        #[case] expected: bool,
    ) {
        let actual = should_report_progress(completed_count, total);
        assert_eq!(expected, actual);
    }
}
