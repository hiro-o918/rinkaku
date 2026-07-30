//! ADR 0062: the pre-analysis update confirmation — offered on the
//! ordinary terminal before `TuiSession::init` opens the alternate
//! screen, rather than as a TUI popup after analysis has already run.
//!
//! Kept next to `self_update.rs` in the bin crate for the same reason
//! that module gives: this is process/network IO tied to how this
//! binary is distributed. The decision of *whether* to ask is a pure
//! function ([`decide_pre_analysis_prompt`]); the channel receive, the
//! terminal read, and the `exec` are the thin IO shell around it.

use crate::self_update::{self, Announcement, UpdateOutcome};
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreAnalysisOutcome<R = Receiver<String>> {
    /// The receiver is handed back when it may still deliver, so the
    /// TUI's status-line hint and `u` key remain the fallback path.
    NotAsked(Option<R>),
    /// The receiver is deliberately dropped, so the TUI cannot ask the
    /// same question a second time in one run.
    Declined,
}

impl PreAnalysisOutcome {
    /// A `Debug + PartialEq` stand-in for whole-value assertions, which
    /// `Receiver` blocks by implementing neither usefully.
    #[cfg(test)]
    fn shape(&self) -> PreAnalysisOutcome<()> {
        match self {
            Self::NotAsked(receiver) => PreAnalysisOutcome::NotAsked(receiver.as_ref().map(|_| ())),
            Self::Declined => PreAnalysisOutcome::Declined,
        }
    }
}

/// Accepting runs the update and, *only if the binary was actually
/// replaced*, re-execs — in which case this never returns. An accepted
/// prompt that ends up not updating (the release vanished, the crate
/// reported no change) falls through to analysis like a declined one.
///
/// `before_reexec` runs on the one path this function does not return
/// from; see [`reexec_current_command`].
pub(crate) fn offer_pre_analysis_update(
    update_check: Option<Receiver<String>>,
    before_reexec: impl FnOnce(),
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
    if !self_update::is_affirmative(&answer) {
        return Ok(PreAnalysisOutcome::Declined);
    }

    update_and_reexec(before_reexec)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AfterUpdate {
    Reexec,
    ContinueToAnalysis,
}

/// Re-execing on a merely *successful* `run_self_update` rather than an
/// *updating* one would loop: the replacement image would be the same
/// version, would find the same release newer, and would ask again.
fn decide_after_update(outcome: UpdateOutcome) -> AfterUpdate {
    match outcome {
        UpdateOutcome::Updated => AfterUpdate::Reexec,
        UpdateOutcome::NotUpdated => AfterUpdate::ContinueToAnalysis,
    }
}

/// `yes: true` since the question was already answered above, and
/// `AlreadyAnnounced` since the version was printed above too.
fn update_and_reexec(before_reexec: impl FnOnce()) -> Result<PreAnalysisOutcome> {
    let outcome = self_update::run_self_update(true, Announcement::AlreadyAnnounced)?;
    match decide_after_update(outcome) {
        AfterUpdate::Reexec => {
            before_reexec();
            reexec_current_command()
        }
        AfterUpdate::ContinueToAnalysis => Ok(PreAnalysisOutcome::NotAsked(None)),
    }
}

/// `exec` replaces the process image, so no `Drop` in this thread's stack
/// ever runs. Whatever needs releasing must be released before this call,
/// which is what `offer_pre_analysis_update`'s `before_reexec` is for.
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
    #[case::should_reexec_when_the_binary_was_replaced(UpdateOutcome::Updated, AfterUpdate::Reexec)]
    #[case::should_continue_to_analysis_when_nothing_was_replaced(
        UpdateOutcome::NotUpdated,
        AfterUpdate::ContinueToAnalysis
    )]
    fn should_decide_what_follows_an_accepted_update(
        #[case] outcome: UpdateOutcome,
        #[case] expected: AfterUpdate,
    ) {
        let actual = decide_after_update(outcome);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_ask_when_there_is_no_version_check_at_all() {
        let actual = offer_pre_analysis_update(None, || {}).expect("offer");

        assert_eq!(PreAnalysisOutcome::NotAsked(None), actual.shape());
    }

    #[test]
    fn should_hand_the_receiver_back_when_the_check_thread_is_still_running() {
        // The sender is kept alive but never sends, so `recv_timeout`
        // takes the `Timeout` branch rather than `Disconnected`.
        let (sender, receiver) = std::sync::mpsc::channel::<String>();

        let started_at = std::time::Instant::now();
        let actual = offer_pre_analysis_update(Some(receiver), || {}).expect("offer");
        let waited = started_at.elapsed();

        assert_eq!(PreAnalysisOutcome::NotAsked(Some(())), actual.shape());
        assert_eq!(true, waited >= CHECK_WAIT, "waited {waited:?}");
        drop(sender);
    }

    #[test]
    fn should_not_hand_the_receiver_back_when_the_check_found_nothing() {
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        drop(sender);

        let actual = offer_pre_analysis_update(Some(receiver), || {}).expect("offer");

        assert_eq!(PreAnalysisOutcome::NotAsked(None), actual.shape());
    }

    // `cargo test` runs with a non-TTY stdin, which is exactly the state
    // this test needs: a version arriving well within `CHECK_WAIT` must
    // still not reach the prompt or the re-exec, since a piped stdin may
    // be carrying the diff itself.
    #[test]
    fn should_not_prompt_when_a_version_arrives_in_time_but_stdin_is_not_a_tty() {
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        sender.send("9.9.9".to_string()).expect("send");

        let actual = offer_pre_analysis_update(Some(receiver), || {}).expect("offer");

        assert_eq!(PreAnalysisOutcome::NotAsked(None), actual.shape());
    }
}
