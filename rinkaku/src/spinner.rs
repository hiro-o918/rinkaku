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

/// Which analysis phase is currently running, decided from the same
/// input-mode branching `main` already does (`--pr` / `--base` / stdin /
/// whole-repo). Kept as its own enum — rather than passing message strings
/// straight from each call site — so the phase → message mapping
/// ([`phase_message`]) is a single pure function callers can unit-test
/// without touching `indicatif`/stderr at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisPhase {
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
