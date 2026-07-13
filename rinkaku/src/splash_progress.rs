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

/// See the module doc comment. Owns the [`TuiSession`] for the duration of
/// the analysis phase; `main.rs` extracts it back out via
/// [`SplashProgress::into_session`] once analysis finishes, to hand off to
/// [`TuiSession::run`]'s main event loop.
pub(crate) struct SplashProgress {
    // `phase_label` is cached alongside the terminal so a file-progress
    // redraw (`report_file_progress`) can still show *which* phase is
    // measuring that progress — `rinkaku_core`'s `on_progress` callback
    // only ever receives `(done, total)`, not a phase label, so without
    // this cache a progress redraw mid-phase would have nothing to put in
    // `SplashState::phase_label`.
    inner: Mutex<(TuiSession, String)>,
}

impl SplashProgress {
    /// `session` must already be initialized (`TuiSession::init`) — this
    /// type only draws on it, it does not own the init/teardown lifecycle
    /// itself (`main.rs` does, via `TuiSession`'s own `Drop`/`run`).
    pub(crate) fn new(session: TuiSession) -> Self {
        Self {
            inner: Mutex::new((session, phase_message(AnalysisPhase::Starting).to_string())),
        }
    }

    /// Unwraps back into the plain [`TuiSession`] once analysis is done,
    /// so `main.rs` can hand it to [`TuiSession::run`] without tearing down
    /// and re-initializing the terminal in between.
    pub(crate) fn into_session(self) -> TuiSession {
        // `.expect()`: a poisoned mutex here means an earlier
        // `set_phase`/`report_file_progress` call panicked while holding
        // the lock — since neither does anything beyond a `Terminal::draw`
        // call and a couple of field writes, that would itself be a bug
        // worth surfacing loudly (a raw panic message) rather than papering
        // over with a fallback session, which `main.rs`'s caller has no way
        // to recover from anyway (the terminal state would already be
        // indeterminate).
        let (session, _label) = self
            .inner
            .into_inner()
            .expect("splash progress mutex must not be poisoned");
        session
    }
}

impl AnalysisProgress for SplashProgress {
    fn set_phase(&self, phase: AnalysisPhase) {
        let label = phase_message(phase).to_string();
        // Same poisoning stance as `into_session`: a draw failure here
        // (`io::Result::Err`) is dropped rather than propagated — this
        // trait's methods return `()` (`AnalysisProgress`'s own doc
        // comment on why: every call site is inside `rinkaku-core`'s
        // callback contract, `Fn(usize, usize)`/phase notification, neither
        // of which is `Result`-returning) — a failed splash redraw is not
        // worth aborting the whole analysis pipeline over, the same
        // judgment call ADR 0032's spinner already makes implicitly by
        // never checking `indicatif`'s own draw failures either.
        let mut guard = self
            .inner
            .lock()
            .expect("splash progress mutex must not be poisoned");
        let (session, cached_label) = &mut *guard;
        cached_label.clone_from(&label);
        let _ = session.draw_splash(&SplashState::label_only(label));
    }

    fn report_file_progress(&self, done: usize, total: usize) {
        let mut guard = self
            .inner
            .lock()
            .expect("splash progress mutex must not be poisoned");
        let (session, cached_label) = &mut *guard;
        let _ = session.draw_splash(&SplashState::with_progress(
            cached_label.clone(),
            done,
            total,
        ));
    }
}
