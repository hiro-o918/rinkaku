//! The `--tui` mode implementation of [`AnalysisProgress`] (ADR 0033):
//! redraws `rinkaku_tui`'s splash screen on the same terminal
//! `TuiSession::init` already opened, in place of ADR 0032's stderr
//! spinner.
//!
//! Wraps the live [`rinkaku_tui::TuiSession`] in a [`std::sync::Mutex`]
//! rather than holding a plain `&mut` — `rinkaku_core::pipeline::analyze_repo`'s
//! `on_progress` callback (ADR 0033) is called from rayon worker threads
//! during the parallel whole-repo parse (ADR 0031), so the closure
//! `main.rs` builds around this type must be `Sync`. No genuine concurrent
//! contention is expected in practice (the stride in
//! `rinkaku_core::progress::should_report_progress` already bounds how
//! often any thread calls in), but the `Mutex` makes the one-terminal-at-a-
//! time invariant a compile-time guarantee rather than an informal
//! assumption about how often two strided calls could land at once.

use crate::progress::AnalysisProgress;
use crate::spinner::{AnalysisPhase, phase_message};
use rinkaku_tui::TuiSession;
use rinkaku_tui::splash::SplashState;
use std::sync::Mutex;

/// The mutable state [`SplashProgress`] guards behind one [`Mutex`] — the
/// live terminal, the currently-cached phase label (so a file-progress
/// redraw can still show *which* phase is measuring that progress, since
/// `rinkaku-core`'s `on_progress` callback only ever receives `(done,
/// total)`), and the buffered notes (ADR 0033's note-deferral decision, see
/// [`AnalysisProgress::note`]'s own doc comment for why raw `eprintln!`
/// during `--tui` mode corrupts the alternate screen).
struct SplashProgressState {
    session: TuiSession,
    phase_label: String,
    buffered_notes: Vec<String>,
}

/// See the module doc comment. Owns the [`TuiSession`] for the duration of
/// the analysis phase; `main.rs` extracts it back out via
/// [`SplashProgress::into_session_and_notes`] once analysis finishes, to
/// hand the terminal off to [`TuiSession::run`]'s main event loop and flush
/// the buffered notes to stderr only after that terminal has torn down the
/// alternate screen.
pub(crate) struct SplashProgress {
    inner: Mutex<SplashProgressState>,
}

impl SplashProgress {
    /// `session` must already be initialized (`TuiSession::init`) — this
    /// type only draws on it, it does not own the init/teardown lifecycle
    /// itself (`main.rs` does, via `TuiSession`'s own `Drop`/`run`).
    pub(crate) fn new(session: TuiSession) -> Self {
        Self {
            inner: Mutex::new(SplashProgressState {
                session,
                phase_label: phase_message(AnalysisPhase::Starting).to_string(),
                buffered_notes: Vec::new(),
            }),
        }
    }

    /// Unwraps back into the plain [`TuiSession`] and the notes buffered
    /// during analysis (in the order [`AnalysisProgress::note`] received
    /// them), once analysis is done — so `main.rs` can hand the session to
    /// [`TuiSession::run`] without tearing down and re-initializing the
    /// terminal in between, and flush the notes to stderr only once that
    /// terminal has actually left the alternate screen (either via
    /// `TuiSession::run`'s own postamble on success, or `TuiSession`'s
    /// `Drop` safety net on an early-return error — both restore the
    /// terminal before `main.rs` can reach the flush call, since the notes
    /// are extracted here, before either teardown path runs, and only
    /// printed by the caller afterward).
    pub(crate) fn into_session_and_notes(self) -> (TuiSession, Vec<String>) {
        // `.expect()`: a poisoned mutex here means an earlier
        // `set_phase`/`report_file_progress`/`note` call panicked while
        // holding the lock — since none of them do anything beyond a
        // `Terminal::draw` call or a `Vec::push`, that would itself be a
        // bug worth surfacing loudly (a raw panic message) rather than
        // papering over with a fallback session, which `main.rs`'s caller
        // has no way to recover from anyway (the terminal state would
        // already be indeterminate).
        let state = self
            .inner
            .into_inner()
            .expect("splash progress mutex must not be poisoned");
        (state.session, state.buffered_notes)
    }
}

impl AnalysisProgress for SplashProgress {
    fn set_phase(&self, phase: AnalysisPhase) {
        let label = phase_message(phase).to_string();
        // Same poisoning stance as `into_session_and_notes`: a draw
        // failure here (`io::Result::Err`) is dropped rather than
        // propagated — this trait's methods return `()`
        // (`AnalysisProgress`'s own doc comment on why: every call site is
        // inside `rinkaku-core`'s callback contract, `Fn(usize, usize)`/
        // phase notification, neither of which is `Result`-returning) — a
        // failed splash redraw is not worth aborting the whole analysis
        // pipeline over, the same judgment call ADR 0032's spinner already
        // makes implicitly by never checking `indicatif`'s own draw
        // failures either.
        let mut guard = self
            .inner
            .lock()
            .expect("splash progress mutex must not be poisoned");
        guard.phase_label.clone_from(&label);
        let _ = guard.session.draw_splash(&SplashState::label_only(label));
    }

    fn report_file_progress(&self, done: usize, total: usize) {
        let mut guard = self
            .inner
            .lock()
            .expect("splash progress mutex must not be poisoned");
        let label = guard.phase_label.clone();
        let _ = guard
            .session
            .draw_splash(&SplashState::with_progress(label, done, total));
    }

    /// Buffers `message` instead of printing it immediately (ADR 0033):
    /// while the splash is on screen, stderr is not being drawn over by
    /// anything a human can watch happen — the *terminal* itself is in the
    /// alternate screen, so a raw `eprintln!` here would write bytes that
    /// land wherever the alternate-screen buffer's cursor currently sits,
    /// corrupting the next redraw (this method's whole reason for
    /// existing — a bug found by dynamic verification, `--tui --base <ref>
    /// --entry <path-matching-nothing>`, see this crate's `main.rs` for the
    /// flush call this buffer feeds into after the terminal has torn down).
    fn note(&self, message: String) {
        let mut guard = self
            .inner
            .lock()
            .expect("splash progress mutex must not be poisoned");
        guard.buffered_notes.push(message);
    }
}

// No unit tests in this module: `TuiSession::init` needs a real terminal
// (ADR 0033's own doc comment on why `TuiSession`'s lifecycle is not
// unit-tested, mirroring ADR 0032's stance on `Spinner`), so a real
// `SplashProgress` cannot be constructed in a unit test — every method here
// is a thin wrapper (a `Mutex::lock` plus a field write/`Vec::push`/
// `TuiSession::draw_splash` call) with no branching logic of its own to
// exercise in isolation. The two things actually worth pinning as pure
// logic — "an `AnalysisProgress` implementer can override `note` to buffer
// instead of printing" and "buffering preserves call order" — are covered
// by `crate::progress::tests::should_buffer_note_instead_of_printing_when_note_is_overridden`,
// which exercises the exact same trait contract this type implements
// without needing a terminal. This type's own behavior is covered by this
// PR's dynamic verification (pty-driven `--tui` runs, see the PR body).
