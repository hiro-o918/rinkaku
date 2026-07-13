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
    pub(crate) fn release(&self, mut destination: W) -> io::Result<()> {
        let mut guard = self
            .state
            .lock()
            .expect("deferred log sink mutex must not be poisoned");
        if let State::Deferring(buffered) = &*guard {
            destination.write_all(buffered)?;
            destination.flush()?;
        }
        *guard = State::Passthrough(destination);
        Ok(())
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
