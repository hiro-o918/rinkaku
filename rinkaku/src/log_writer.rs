//! A [`std::io::Write`] sink for `env_logger` that defers writes instead of
//! printing them immediately (ADR 0033 amendment): `--tui` mode's alternate
//! screen is the same physical terminal stderr writes to, so a `log::info!`/
//! `log::warn!` fired while the splash/entry screen is being redrawn lands
//! as raw bytes mid-frame, corrupting it — the same failure mode ADR 0033
//! already fixed for `AnalysisProgress::note`, but that fix does not cover
//! `log::` records, which bypass `AnalysisProgress` entirely and go straight
//! to `env_logger`'s own configured target.
//!
//! [`DeferredLogSink`] starts in the deferring state (bytes are appended to
//! an internal buffer) and is flipped to passthrough once, via
//! [`DeferredLogSink::release`], after the terminal has left the alternate
//! screen — draining the buffer to the destination first so log order is
//! preserved, then writing straight through for any record logged after
//! that point.
//!
//! [`ReleaseGuard`] is the panic/early-return safety net for the same
//! release: `main.rs` calls `release` explicitly at its two normal-flow
//! points (for deterministic ordering against the buffered notes flush,
//! see `main.rs`'s own comments), but neither an early `?` return nor a
//! panic unwinding through `run_analysis`/`TuiSession::run` reaches those
//! calls. `ReleaseGuard::drop` releases the sink unconditionally, so a
//! record buffered before either kind of abrupt exit still reaches stderr
//! instead of being silently discarded.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

enum State<W> {
    Deferring(Vec<u8>),
    Passthrough(W),
}

/// Cheaply cloneable handle around a shared [`Write`] destination `W` that
/// starts in the deferring state. Clones share the same underlying buffer —
/// `env_logger::Target::Pipe` takes ownership of one handle to write log
/// records through, while `main.rs` keeps another to call [`release`] once
/// the TUI's terminal has torn down.
///
/// [`release`]: DeferredLogSink::release
pub(crate) struct DeferredLogSink<W> {
    state: Arc<Mutex<State<W>>>,
}

impl<W> Clone for DeferredLogSink<W> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<W: Write> DeferredLogSink<W> {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Deferring(Vec::new()))),
        }
    }

    /// Drains any buffered bytes to `destination` (in the order they were
    /// written) and switches to passthrough: every write after this call
    /// goes straight to `destination` instead of being buffered.
    ///
    /// A no-op if already in the passthrough state (idempotent): the
    /// explicit `main.rs` call and [`ReleaseGuard`]'s `Drop`-based call can
    /// both reach the same sink on a given run, and only the first one
    /// should have any effect.
    pub(crate) fn release(&self, mut destination: W) -> io::Result<()> {
        let mut guard = self
            .state
            .lock()
            .expect("deferred log sink mutex must not be poisoned");
        let State::Deferring(buffered) = &*guard else {
            return Ok(());
        };
        destination.write_all(buffered)?;
        destination.flush()?;
        *guard = State::Passthrough(destination);
        Ok(())
    }
}

/// RAII safety net for [`DeferredLogSink::release`] (see the module doc
/// comment): releases the wrapped sink to `destination` when dropped,
/// covering panics and early `?` returns that never reach `main.rs`'s
/// explicit `release` calls. `destination` is built lazily (`F: FnOnce() ->
/// W`) so it is only constructed if `Drop` actually runs the release —
/// `main.rs` uses this to build a fresh `std::io::Stderr` handle at drop
/// time rather than holding one for the guard's entire lifetime.
pub(crate) struct ReleaseGuard<W: Write, F: FnOnce() -> W> {
    sink: DeferredLogSink<W>,
    destination: Option<F>,
}

impl<W: Write, F: FnOnce() -> W> ReleaseGuard<W, F> {
    pub(crate) fn new(sink: DeferredLogSink<W>, destination: F) -> Self {
        Self {
            sink,
            destination: Some(destination),
        }
    }
}

impl<W: Write, F: FnOnce() -> W> Drop for ReleaseGuard<W, F> {
    fn drop(&mut self) {
        if let Some(destination) = self.destination.take() {
            let _ = self.sink.release(destination());
        }
    }
}

impl<W: Write> Write for DeferredLogSink<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut guard = self
            .state
            .lock()
            .expect("deferred log sink mutex must not be poisoned");
        match &mut *guard {
            State::Deferring(buffered) => buffered.extend_from_slice(buf),
            State::Passthrough(destination) => return destination.write(buf),
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut guard = self
            .state
            .lock()
            .expect("deferred log sink mutex must not be poisoned");
        if let State::Passthrough(destination) = &mut *guard {
            return destination.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    fn should_buffer_writes_when_deferring() {
        let mut sink = DeferredLogSink::new();
        sink.write_all(b"first ").unwrap();
        sink.write_all(b"second").unwrap();

        let destination = Vec::new();
        sink.release(destination.clone()).unwrap();

        assert_eq!(Vec::<u8>::new(), destination);
    }

    #[rstest]
    fn should_drain_buffered_bytes_in_order_when_released() {
        let mut sink = DeferredLogSink::new();
        sink.write_all(b"first ").unwrap();
        sink.write_all(b"second").unwrap();

        let destination: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        sink.release(SharedVec(Arc::clone(&destination))).unwrap();

        let actual = destination.lock().unwrap().clone();
        assert_eq!(b"first second".to_vec(), actual);
    }

    #[rstest]
    fn should_pass_writes_through_when_released() {
        let mut sink = DeferredLogSink::new();
        sink.write_all(b"buffered ").unwrap();

        let destination: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        sink.release(SharedVec(Arc::clone(&destination))).unwrap();
        sink.write_all(b"live").unwrap();

        let actual = destination.lock().unwrap().clone();
        assert_eq!(b"buffered live".to_vec(), actual);
    }

    #[rstest]
    fn should_ignore_second_release_when_already_released() {
        let mut sink = DeferredLogSink::new();
        sink.write_all(b"buffered").unwrap();

        let first: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        sink.release(SharedVec(Arc::clone(&first))).unwrap();

        let second: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        sink.release(SharedVec(Arc::clone(&second))).unwrap();

        let actual_first = first.lock().unwrap().clone();
        let actual_second = second.lock().unwrap().clone();
        assert_eq!(b"buffered".to_vec(), actual_first);
        assert_eq!(Vec::<u8>::new(), actual_second);
    }

    #[rstest]
    fn should_release_buffered_bytes_when_guard_is_dropped() {
        let mut sink = DeferredLogSink::new();
        sink.write_all(b"buffered").unwrap();

        let destination: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let destination_for_guard = Arc::clone(&destination);
        let guard = ReleaseGuard::new(sink, move || SharedVec(destination_for_guard));

        drop(guard);

        let actual = destination.lock().unwrap().clone();
        assert_eq!(b"buffered".to_vec(), actual);
    }

    #[rstest]
    fn should_not_release_when_guard_is_defused_by_explicit_release() {
        let mut sink = DeferredLogSink::new();
        sink.write_all(b"buffered").unwrap();

        let explicit: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        sink.release(SharedVec(Arc::clone(&explicit))).unwrap();

        let guard_destination: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let guard_destination_for_guard = Arc::clone(&guard_destination);
        let guard = ReleaseGuard::new(sink, move || SharedVec(guard_destination_for_guard));
        drop(guard);

        let actual_explicit = explicit.lock().unwrap().clone();
        let actual_guard = guard_destination.lock().unwrap().clone();
        assert_eq!(b"buffered".to_vec(), actual_explicit);
        assert_eq!(Vec::<u8>::new(), actual_guard);
    }

    #[rstest]
    fn should_share_state_across_clones() {
        let sink = DeferredLogSink::new();
        let mut handle_a = sink.clone();
        let mut handle_b = sink.clone();
        handle_a.write_all(b"from a, ").unwrap();
        handle_b.write_all(b"from b").unwrap();

        let destination: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        sink.release(SharedVec(Arc::clone(&destination))).unwrap();

        let actual = destination.lock().unwrap().clone();
        assert_eq!(b"from a, from b".to_vec(), actual);
    }

    /// A [`Write`] destination that appends to a shared buffer, so a test
    /// can both hand ownership to [`DeferredLogSink::release`] and inspect
    /// what was written afterward.
    #[derive(Clone)]
    struct SharedVec(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedVec {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
