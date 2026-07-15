//! `perform_export` clipboard-arm tests (ADR 0048 sink B): the port's own
//! `Ok` status line is surfaced verbatim, its `Err` is wrapped in the
//! error prefix.

use crate::ReviewPorts;
use crate::review::ports::ClipboardSink;
use crate::review::{ExportRequest, ReviewState};
use crate::review_flow::perform_export;
use pretty_assertions::assert_eq;

struct FakeClipboard {
    result: Result<String, String>,
}

impl ClipboardSink for FakeClipboard {
    fn copy(&self, _text: &str) -> Result<String, String> {
        self.result.clone()
    }
}

fn ports_with<'a>(
    clipboard: &'a FakeClipboard,
    browser: &'a super::FakeBrowserOpener,
) -> ReviewPorts<'a> {
    ReviewPorts {
        pr_context: None,
        submitter: None,
        clipboard,
        browser,
    }
}

#[test]
fn should_surface_the_ports_status_line_when_copy_succeeds() {
    let clipboard = FakeClipboard {
        result: Ok("copied review annotations to clipboard via pbcopy".to_string()),
    };
    let browser = super::FakeBrowserOpener::new(Ok(()));

    let actual = perform_export(
        ReviewState::default(),
        &ports_with(&clipboard, &browser),
        ExportRequest::Clipboard,
    );

    assert_eq!(
        Some("copied review annotations to clipboard via pbcopy"),
        actual.last_status()
    );
}

#[test]
fn should_wrap_the_ports_error_in_the_error_prefix_when_copy_fails() {
    let clipboard = FakeClipboard {
        result: Err("no tty".to_string()),
    };
    let browser = super::FakeBrowserOpener::new(Ok(()));

    let actual = perform_export(
        ReviewState::default(),
        &ports_with(&clipboard, &browser),
        ExportRequest::Clipboard,
    );

    assert_eq!(
        Some("error copying to clipboard: no tty"),
        actual.last_status()
    );
}
