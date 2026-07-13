//! A stderr progress spinner shown while `main` runs the analysis pipeline
//! (`analyze_repo`/`build_resolver`/`analyze_diff`/`run_git_diff`) before
//! either the TUI starts or Markdown/JSON output is printed.
//!
//! This exists because that pipeline is entirely synchronous and, for a
//! large repository or a PR with many files, can take from hundreds of
//! milliseconds to several seconds (ADR 0031's profiling table) with no
//! terminal feedback at all in between — the process just appears to hang.
//! ADR 0032 records why a spinner (rather than ADR 0031's deferred lazy
//! start / progressive rendering alternatives) was chosen for this.
//!
//! Kept in the `rinkaku` bin crate, not `rinkaku-core`: this is terminal IO
//! tied to how *this specific binary* reports progress, not part of the
//! pure diff-condensation core (CLAUDE.md's "core logic is pure" rule).
//!
//! Used only outside `--tui` mode (ADR 0033): `main.rs` shows this spinner
//! or the TUI splash screen (`rinkaku_tui::splash`), never both in the same
//! run — see `main.rs`'s own doc comment on why display mode is decided
//! before analysis starts.

use crate::progress::AnalysisProgress;
use indicatif::{ProgressBar, ProgressStyle};

/// Wraps an `indicatif::ProgressBar` configured as an indeterminate spinner
/// on stderr. `indicatif`'s `Term`-backed stderr draw target already
/// detects non-TTY stderr (piped/redirected) and suppresses all drawing in
/// that case — see `ProgressDrawTarget::stderr`'s own doc comment — so no
/// explicit `IsTerminal` check is needed here; this type only has to get
/// the *style* and *message* right, not the TTY decision.
pub struct Spinner {
    bar: ProgressBar,
}

impl Spinner {
    /// Starts a new spinner on stderr with `message` as its initial phase
    /// label (see [`phase_message`]).
    pub fn start(message: &str) -> Self {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            // `with_template` only fails on a malformed template string
            // (unknown placeholder, unbalanced braces); the template here
            // is a fixed literal using two placeholders documented in
            // `indicatif::ProgressStyle`'s own reference, so this can never
            // fail at runtime — the `expect` exists to satisfy the
            // `Result`-returning signature, not to guard a real failure
            // mode.
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .expect("static spinner template is valid"),
        );
        bar.enable_steady_tick(std::time::Duration::from_millis(100));
        bar.set_message(message.to_string());
        Self { bar }
    }

    /// Updates the spinner's message in place (e.g. moving from "resolving
    /// PR" to "analyzing diff") without starting a new spinner line.
    pub fn set_message(&self, message: &str) {
        self.bar.set_message(message.to_string());
    }

    /// Clears the spinner from the terminal. Must be called before printing
    /// anything else to stdout/stderr that should not be interleaved with
    /// spinner redraws, and in particular before the TUI takes over the
    /// terminal (entering the alternate screen with a stray spinner line
    /// still drawn would corrupt the TUI's first frame).
    pub fn finish_and_clear(&self) {
        self.bar.finish_and_clear();
    }
}

impl AnalysisProgress for Spinner {
    /// Forwards to [`Spinner::set_message`] via [`phase_message`] — the
    /// same mapping every call site used before `AnalysisProgress` existed.
    fn set_phase(&self, phase: AnalysisPhase) {
        self.set_message(phase_message(phase));
    }

    // `report_file_progress` is left at `AnalysisProgress`'s default no-op:
    // ADR 0032 chose an indeterminate spinner on purpose (see this module's
    // own doc comment), so a `(done, total)` count arriving here is
    // intentionally dropped rather than reformatted into the spinner's
    // single message line.
}

// Note: an early `?`-propagated error in `main` (e.g. a failing `git`/`gh`
// call inside `run_base_pipeline`/`build_resolver`) drops the `Spinner`
// without ever reaching a `finish_and_clear()` call. This is still safe:
// `indicatif`'s underlying `BarState` clears the line on `Drop` unless the
// bar was already finished, using `ProgressFinish::AndClear` — the crate's
// documented default — so the spinner never survives past the process
// printing its error to stderr, with or without an explicit clear call.

/// Which analysis phase is currently running, decided from the same
/// input-mode branching `main` already does (`--pr` / `--base` / stdin /
/// whole-repo). Kept as its own enum — rather than passing message strings
/// straight from each call site — so the phase → message mapping
/// ([`phase_message`]) is a single pure function callers can unit-test
/// without touching `indicatif`/stderr at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisPhase {
    /// The spinner's initial state, shown for the brief window between
    /// `Spinner::start` and `main` determining which input mode (`--pr` /
    /// `--base` / stdin / whole-repo) actually applies — none of the other
    /// variants is correct yet at that point, so this exists to avoid
    /// mislabeling that window as e.g. "Analyzing diff...".
    Starting,
    /// Resolving a `--pr` argument via `gh` (fetching PR metadata, base/head
    /// commits) before any diffing starts.
    ResolvingPr,
    /// Running `git diff <base>...<head>` (`--base`/`--pr` mode).
    Diffing,
    /// Building the repo-wide dependency index (`build_resolver`, `--deps
    /// 1`, the default).
    BuildingDependencyIndex,
    /// Parsing every tracked file for the whole-repo outline
    /// (`analyze_repo`, ADR 0017's default when stdin is a terminal).
    ParsingRepository,
    /// Slicing signatures out of the changed files (`analyze_diff`).
    AnalyzingDiff,
}

/// The stderr message for `phase`. Pure mapping, split out from
/// [`Spinner::set_message`]'s call sites so it is unit-testable without a
/// real terminal.
pub fn phase_message(phase: AnalysisPhase) -> &'static str {
    match phase {
        AnalysisPhase::Starting => "Starting...",
        AnalysisPhase::ResolvingPr => "Resolving PR...",
        AnalysisPhase::Diffing => "Diffing...",
        AnalysisPhase::BuildingDependencyIndex => "Building dependency index...",
        AnalysisPhase::ParsingRepository => "Parsing repository...",
        AnalysisPhase::AnalyzingDiff => "Analyzing diff...",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_return_starting_message_when_phase_is_starting(
        AnalysisPhase::Starting,
        "Starting..."
    )]
    #[case::should_return_resolving_pr_message_when_phase_is_resolving_pr(
        AnalysisPhase::ResolvingPr,
        "Resolving PR..."
    )]
    #[case::should_return_diffing_message_when_phase_is_diffing(
        AnalysisPhase::Diffing,
        "Diffing..."
    )]
    #[case::should_return_dependency_index_message_when_phase_is_building_dependency_index(
        AnalysisPhase::BuildingDependencyIndex,
        "Building dependency index..."
    )]
    #[case::should_return_parsing_repository_message_when_phase_is_parsing_repository(
        AnalysisPhase::ParsingRepository,
        "Parsing repository..."
    )]
    #[case::should_return_analyzing_diff_message_when_phase_is_analyzing_diff(
        AnalysisPhase::AnalyzingDiff,
        "Analyzing diff..."
    )]
    fn phase_message_returns_expected_text_per_phase(
        #[case] phase: AnalysisPhase,
        #[case] expected: &str,
    ) {
        let actual = phase_message(phase);
        assert_eq!(expected, actual);
    }
}
