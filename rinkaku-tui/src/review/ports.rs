//! Side-effect ports for review-notes export (ADR 0048's Output boundary
//! decision): `review` returns only plain data, never calling `gh` or
//! touching the clipboard itself — every side effect sits behind one of
//! these traits, defined here (the consumer side, per this project's
//! CLAUDE.md "ports as traits, defined where consumed" principle) and
//! implemented by the `rinkaku` binary crate's composition root
//! (`main.rs`).

use super::{PrContext, RenderedComment, Verdict};

/// Posts a batch of review comments as one GitHub PR review (ADR 0048 sink
/// A) — the pending-review shape (open, attach comments, submit with a
/// verdict) rather than one call per comment, so the reviewer can discard
/// the whole batch by never confirming the verdict menu.
pub trait ReviewSubmitter {
    fn submit_review(
        &self,
        ctx: &PrContext,
        verdict: Verdict,
        summary: &str,
        comments: &[RenderedComment],
    ) -> Result<(), String>;
}

/// Writes `text` to the system clipboard (ADR 0048 sink B), best-effort —
/// see the ADR's Alternatives on why this is OSC 52 rather than a
/// clipboard crate, and why a successful `Ok(())` here does not guarantee
/// the terminal actually populated the clipboard.
pub trait ClipboardSink {
    fn copy(&self, text: &str) -> Result<(), String>;
}
