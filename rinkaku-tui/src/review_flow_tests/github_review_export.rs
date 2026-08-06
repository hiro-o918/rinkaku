//! `perform_export`'s [`ExportRequest::GithubReview`] arm (ADR 0067):
//! anchored annotations become inline `RenderedComment`s exactly as before
//! (ADR 0048), and any unanchored annotation (a `File`/`Dir`/`RemovedSymbol`
//! target, or a `Symbol` target whose range never intersected a hunk) is
//! folded into an "Additional notes" section appended to the fixed review
//! summary — pinning the exact composed `summary` string
//! [`ReviewSubmitter::submit_review`] is called with.

use crate::ReviewPorts;
use crate::review::ports::ReviewSubmitter;
use crate::review::{
    AnnotationTarget, ExportRequest, PrContext, RenderedComment, ReviewState, SelectionSnapshot,
    Verdict,
};
use crate::review_flow::perform_export;
use pretty_assertions::assert_eq;

struct RecordingSubmitter {
    calls: std::cell::RefCell<Vec<(String, Vec<RenderedComment>)>>,
    result: Result<(), String>,
}

impl RecordingSubmitter {
    fn new(result: Result<(), String>) -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            result,
        }
    }
}

impl ReviewSubmitter for RecordingSubmitter {
    fn submit_review(
        &self,
        _ctx: &PrContext,
        _verdict: Verdict,
        summary: &str,
        comments: &[RenderedComment],
    ) -> Result<(), String> {
        self.calls
            .borrow_mut()
            .push((summary.to_string(), comments.to_vec()));
        self.result.clone()
    }
}

fn pr_context() -> PrContext {
    PrContext {
        owner: "octocat".to_string(),
        repo: "hello-world".to_string(),
        number: 42,
        head_sha: "abc123".to_string(),
    }
}

struct UnusedClipboard;
impl crate::review::ports::ClipboardSink for UnusedClipboard {
    fn copy(&self, _text: &str) -> Result<String, String> {
        panic!("clipboard should not be used by the GithubReview export arm")
    }
}

fn ports_with<'a>(
    submitter: &'a RecordingSubmitter,
    browser: &'a super::FakeBrowserOpener,
    clipboard: &'a UnusedClipboard,
) -> ReviewPorts<'a> {
    ReviewPorts {
        pr_context: Some(pr_context()),
        submitter: Some(submitter),
        clipboard,
        browser,
    }
}

fn symbol_snapshot(anchor: (usize, usize)) -> SelectionSnapshot {
    SelectionSnapshot {
        target: AnnotationTarget::Symbol,
        path: "src/lib.rs".to_string(),
        symbol_id: Some("src/lib.rs::foo".to_string()),
        symbol_name: Some("foo".to_string()),
        range: Some(anchor),
        anchor: Some(anchor),
        signature: Some("fn foo()".to_string()),
    }
}

fn file_snapshot() -> SelectionSnapshot {
    SelectionSnapshot {
        target: AnnotationTarget::File,
        path: "src/dead_code.rs".to_string(),
        symbol_id: None,
        symbol_name: None,
        range: None,
        anchor: None,
        signature: None,
    }
}

/// Composes one annotation from `snapshot` with body `body`, via the same
/// `begin_compose`/`push_char`/`confirm_compose` sequence `App::handle_key`
/// drives in production — kept as a helper so this module's tests build
/// multi-annotation batches without reaching into `ReviewState`'s private
/// fields.
fn compose(review: ReviewState, snapshot: SelectionSnapshot, body: &str) -> ReviewState {
    let mut review = review.begin_compose(snapshot);
    for c in body.chars() {
        review = review.push_char(c);
    }
    review.confirm_compose()
}

#[test]
fn should_post_only_the_fixed_summary_when_every_annotation_is_anchored() {
    let submitter = RecordingSubmitter::new(Ok(()));
    let browser = super::FakeBrowserOpener::new(Ok(()));
    let clipboard = UnusedClipboard;
    let review = compose(
        ReviewState::default(),
        symbol_snapshot((10, 10)),
        "please add a test",
    );

    let actual = perform_export(
        review,
        &ports_with(&submitter, &browser, &clipboard),
        ExportRequest::GithubReview(Verdict::Approve),
    );

    assert_eq!(
        vec![(
            "Review annotations posted via rinkaku.".to_string(),
            vec![RenderedComment {
                path: "src/lib.rs".to_string(),
                line: 10,
                start_line: None,
                body: "please add a test".to_string(),
            }],
        )],
        *submitter.calls.borrow()
    );
    assert_eq!(
        Some("posted 1 review comment(s) to PR #42"),
        actual.last_status()
    );
}

#[test]
fn should_append_additional_notes_section_when_an_unanchored_annotation_is_present() {
    let submitter = RecordingSubmitter::new(Ok(()));
    let browser = super::FakeBrowserOpener::new(Ok(()));
    let clipboard = UnusedClipboard;
    let review = compose(
        ReviewState::default(),
        symbol_snapshot((10, 10)),
        "please add a test",
    );
    let review = compose(review, file_snapshot(), "this whole file is dead code now");

    let actual = perform_export(
        review,
        &ports_with(&submitter, &browser, &clipboard),
        ExportRequest::GithubReview(Verdict::Comment),
    );

    assert_eq!(
        vec![(
            "Review annotations posted via rinkaku.\n\n\
             ## Additional notes\n\
             - `src/dead_code.rs`: this whole file is dead code now\n"
                .to_string(),
            vec![RenderedComment {
                path: "src/lib.rs".to_string(),
                line: 10,
                start_line: None,
                body: "please add a test".to_string(),
            }],
        )],
        *submitter.calls.borrow()
    );
    assert_eq!(
        Some("posted 1 review comment(s) to PR #42"),
        actual.last_status()
    );
}
