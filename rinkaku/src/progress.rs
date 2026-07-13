//! The pre-render analysis progress port `main.rs` threads through
//! `pipeline::run_base_pipeline`/`build_resolver` (ADR 0033), replacing the
//! bare `&Spinner` parameter those functions used to take.
//!
//! Two phase-reporting concerns used to be conflated in `&Spinner`: "which
//! phase is running" (every display mode wants this) and "how far along a
//! file-scanning phase is" (only `--tui` mode's splash screen can show a
//! real bar for — ADR 0032's stderr spinner is deliberately indeterminate).
//! [`AnalysisProgress`] separates them into two methods so a caller that has
//! no file-count signal (the stderr [`crate::spinner::Spinner`]) can just
//! no-op the second one, rather than every display mode having to fake a
//! `(done, total)` pair it doesn't have.
//!
//! Kept small (two methods) and defined here, on the consumer side
//! (`pipeline.rs` is what actually calls through this port) per CLAUDE.md's
//! port convention.

use crate::spinner::AnalysisPhase;

/// What `pipeline::run_base_pipeline`/`build_resolver` report through as
/// the analysis pipeline progresses, decoupling them from which concrete
/// display mode is running (ADR 0032's stderr spinner vs. ADR 0033's TUI
/// splash screen — exactly one of the two is active per run, `main.rs`'s
/// own doc comment on why).
///
/// `Sync`: `report_file_progress` is called through
/// `rinkaku_core::pipeline::analyze_repo`'s `on_progress` port from rayon
/// worker threads during the parallel whole-repo parse (ADR 0031), so a
/// `&dyn AnalysisProgress` reference must be safe to share across those
/// threads — the same reasoning `rinkaku_core::progress::OnProgress`'s own
/// doc comment gives for its `Sync` bound.
pub(crate) trait AnalysisProgress: Sync {
    /// Called when the pipeline moves into a new named phase (resolving a
    /// PR, diffing, building the dependency index, analyzing the diff) —
    /// mirrors the granularity `Spinner::set_message`/`phase_message`
    /// already established under ADR 0032.
    fn set_phase(&self, phase: AnalysisPhase);

    /// Called with `(files_done, total)` while a file-scanning phase (the
    /// dependency index build) is in progress. A no-op default: only a
    /// caller that can actually render a determinate bar (the TUI splash)
    /// needs to override this — the stderr spinner stays indeterminate
    /// (ADR 0032), so its impl leaves this at the default no-op.
    fn report_file_progress(&self, _done: usize, _total: usize) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

    // A minimal fake exercising the default `report_file_progress` no-op —
    // this project's "no mocking of external processes" rule does not
    // apply here (no process/terminal is touched), but there is also
    // nothing to fake beyond recording calls, so a plain hand-rolled fake
    // is used rather than a mocking framework. `Mutex` rather than
    // `RefCell`: `AnalysisProgress: Sync` (this module's own doc comment
    // on why), so any implementer — including this test fake — must be
    // `Sync` too, which `RefCell` is not.
    struct RecordingProgress {
        phases: Mutex<Vec<AnalysisPhase>>,
    }

    impl AnalysisProgress for RecordingProgress {
        fn set_phase(&self, phase: AnalysisPhase) {
            self.phases
                .lock()
                .expect("lock must not be poisoned")
                .push(phase);
        }
    }

    #[test]
    fn should_record_phase_and_no_op_file_progress_when_using_default_impl() {
        let progress = RecordingProgress {
            phases: Mutex::new(Vec::new()),
        };

        progress.set_phase(AnalysisPhase::Diffing);
        // Must not panic and must not require an override — this is the
        // exact contract the default no-op body exists to provide.
        progress.report_file_progress(3, 10);

        let expected = vec![AnalysisPhase::Diffing];
        let actual = progress
            .phases
            .into_inner()
            .expect("lock must not be poisoned");
        assert_eq!(expected, actual);
    }
}
