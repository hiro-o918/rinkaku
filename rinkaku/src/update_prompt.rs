//! ADR 0062: the pre-analysis update confirmation — offered on the
//! ordinary terminal before `TuiSession::init` opens the alternate
//! screen, rather than as a TUI popup after analysis has already run.
//!
//! Kept next to `self_update.rs` in the bin crate for the same reason
//! that module gives: this is process/network IO tied to how this
//! binary is distributed. The decision of *whether* to ask is a pure
//! function ([`decide_pre_analysis_prompt`]); the channel receive, the
//! terminal read, and the `exec` are the thin IO shell around it.

use crate::self_update;
use anyhow::Result;
use std::io::{IsTerminal, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

/// How long `main` blocks on the background version check before giving
/// up and starting analysis. ADR 0062: ~4x the measured ~0.07s cost of
/// the check, small enough that a slow network cannot make startup feel
/// worse than not checking at all.
const CHECK_WAIT: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreAnalysisPrompt {
    Ask,
    Skip,
}

/// The `stdin_is_tty` input carries more weight than "there is nobody to
/// answer": a non-TTY stdin is very likely the diff itself
/// (`gh pr diff 123 | rinkaku`), which a post-update re-exec could never
/// read a second time. Refusing to ask here is what makes that case
/// structurally unreachable rather than a special case downstream.
fn decide_pre_analysis_prompt(update_available: bool, stdin_is_tty: bool) -> PreAnalysisPrompt {
    if update_available && stdin_is_tty {
        PreAnalysisPrompt::Ask
    } else {
        PreAnalysisPrompt::Skip
    }
}

/// Requires an explicit `y`/`yes`, matching `self_update`'s own
/// `is_affirmative`, so a stray newline can never be read as consent to
/// replace the running binary.
fn is_affirmative(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

pub(crate) enum PreAnalysisOutcome {
    /// The receiver is handed back when it may still deliver, so the
    /// TUI's status-line hint and `u` key remain the fallback path.
    NotAsked(Option<Receiver<String>>),
    /// The receiver is deliberately dropped, so the TUI cannot ask the
    /// same question a second time in one run.
    Declined,
}

/// Accepting never returns: the update runs and the process re-execs
/// itself (see [`update_and_reexec`]).
pub(crate) fn offer_pre_analysis_update(
    update_check: Option<Receiver<String>>,
) -> Result<PreAnalysisOutcome> {
    let Some(receiver) = update_check else {
        return Ok(PreAnalysisOutcome::NotAsked(None));
    };
    let version = match receiver.recv_timeout(CHECK_WAIT) {
        Ok(version) => version,
        Err(RecvTimeoutError::Timeout) => {
            return Ok(PreAnalysisOutcome::NotAsked(Some(receiver)));
        }
        Err(RecvTimeoutError::Disconnected) => {
            return Ok(PreAnalysisOutcome::NotAsked(None));
        }
    };
    if decide_pre_analysis_prompt(true, std::io::stdin().is_terminal()) == PreAnalysisPrompt::Skip {
        return Ok(PreAnalysisOutcome::NotAsked(None));
    }

    let current_version = env!("CARGO_PKG_VERSION");
    println!("New release found: v{current_version} -> v{version}");
    print!("Update to v{version} and re-run? [y/N]: ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !is_affirmative(&answer) {
        return Ok(PreAnalysisOutcome::Declined);
    }

    update_and_reexec()
}

/// `yes: true` since the question was already answered above. On success
/// the `exec` never returns, so there is no re-check loop to guard
/// against: the replacement image is a newer version, and its own check
/// finds nothing newer.
fn update_and_reexec() -> Result<PreAnalysisOutcome> {
    self_update::run_self_update(true)?;
    reexec_current_command()
}

#[cfg(unix)]
fn reexec_current_command() -> Result<PreAnalysisOutcome> {
    use std::os::unix::process::CommandExt;

    let exe = std::env::current_exe()?;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    Err(anyhow::Error::from(
        std::process::Command::new(exe).args(args).exec(),
    ))
}

#[cfg(not(unix))]
fn reexec_current_command() -> Result<PreAnalysisOutcome> {
    println!("Re-run rinkaku to analyze with the updated binary");
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_ask_when_an_update_is_available_and_stdin_is_a_tty(
        true,
        true,
        PreAnalysisPrompt::Ask
    )]
    #[case::should_skip_when_no_update_is_available(false, true, PreAnalysisPrompt::Skip)]
    #[case::should_skip_when_stdin_is_not_a_tty(true, false, PreAnalysisPrompt::Skip)]
    #[case::should_skip_when_no_update_is_available_and_stdin_is_not_a_tty(
        false,
        false,
        PreAnalysisPrompt::Skip
    )]
    fn should_decide_pre_analysis_prompt(
        #[case] update_available: bool,
        #[case] stdin_is_tty: bool,
        #[case] expected: PreAnalysisPrompt,
    ) {
        let actual = decide_pre_analysis_prompt(update_available, stdin_is_tty);

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_accept_lowercase_y("y", true)]
    #[case::should_accept_uppercase_y("Y", true)]
    #[case::should_accept_lowercase_yes("yes", true)]
    #[case::should_accept_mixed_case_yes("Yes", true)]
    #[case::should_accept_y_with_surrounding_whitespace("  y  \n", true)]
    #[case::should_reject_empty_string("", false)]
    #[case::should_reject_whitespace_only_string("   \n", false)]
    #[case::should_reject_n("n", false)]
    #[case::should_reject_no("no", false)]
    #[case::should_reject_y_as_prefix_of_longer_word("yesterday", false)]
    fn should_check_is_affirmative(#[case] answer: &str, #[case] expected: bool) {
        let actual = is_affirmative(answer);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_wait_at_most_300ms_for_the_background_check() {
        assert_eq!(Duration::from_millis(300), CHECK_WAIT);
    }

    #[test]
    fn should_not_ask_when_there_is_no_version_check_at_all() {
        let actual = offer_pre_analysis_update(None);

        assert_eq!(
            true,
            matches!(actual, Ok(PreAnalysisOutcome::NotAsked(None)))
        );
    }

    #[test]
    fn should_hand_the_receiver_back_when_the_check_thread_is_still_running() {
        // The sender is kept alive but never sends, so `recv_timeout`
        // takes the `Timeout` branch rather than `Disconnected`.
        let (sender, receiver) = std::sync::mpsc::channel::<String>();

        let actual = offer_pre_analysis_update(Some(receiver));

        assert_eq!(
            true,
            matches!(actual, Ok(PreAnalysisOutcome::NotAsked(Some(_))))
        );
        drop(sender);
    }

    #[test]
    fn should_not_hand_the_receiver_back_when_the_check_found_nothing() {
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        drop(sender);

        let actual = offer_pre_analysis_update(Some(receiver));

        assert_eq!(
            true,
            matches!(actual, Ok(PreAnalysisOutcome::NotAsked(None)))
        );
    }

    // `cargo test` runs with a non-TTY stdin, which is exactly the state
    // this test needs: a version arriving well within `CHECK_WAIT` must
    // still not reach the prompt or the re-exec, since a piped stdin may
    // be carrying the diff itself.
    #[test]
    fn should_not_prompt_when_a_version_arrives_in_time_but_stdin_is_not_a_tty() {
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        sender.send("9.9.9".to_string()).expect("send");

        let actual = offer_pre_analysis_update(Some(receiver));

        assert_eq!(
            true,
            matches!(actual, Ok(PreAnalysisOutcome::NotAsked(None)))
        );
    }
}
