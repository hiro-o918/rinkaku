//! `open_pr_in_browser` tests (ADR 0050): the no-`PrContext` and
//! spawn-failure status-line messages, and the URL actually passed to
//! [`crate::review::ports::BrowserOpener`] when a `PrContext` is present.

use super::FakeBrowserOpener;
use super::empty_report;
use crate::ReviewPorts;
use crate::app::App;
use crate::review::PrContext;
use crate::review::ports::ClipboardSink;
use crate::review_flow::open_pr_in_browser;
use pretty_assertions::assert_eq;

struct NoopClipboard;

impl ClipboardSink for NoopClipboard {
    fn copy(&self, _text: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

fn ports_with<'a>(
    pr_context: Option<PrContext>,
    clipboard: &'a NoopClipboard,
    browser: &'a FakeBrowserOpener,
) -> ReviewPorts<'a> {
    ReviewPorts {
        pr_context,
        submitter: None,
        clipboard,
        browser,
    }
}

fn pr_context() -> PrContext {
    PrContext {
        owner: "hiro-o918".to_string(),
        repo: "rinkaku".to_string(),
        number: 42,
        head_sha: "deadbeef".to_string(),
    }
}

#[test]
fn should_set_a_status_message_when_no_pr_context_is_available() {
    let report = empty_report();
    let app = App::new(&report);
    let clipboard = NoopClipboard;
    let browser = FakeBrowserOpener::new(Ok(()));
    let ports = ports_with(None, &clipboard, &browser);

    let actual = open_pr_in_browser(app, &ports);

    assert_eq!(
        Some("note: no PR context available to open a browser (not running in --pr mode)"),
        actual.status()
    );
    assert_eq!(None, *browser.opened_url.borrow());
}

#[test]
fn should_open_the_pr_page_url_when_a_pr_context_is_available() {
    let report = empty_report();
    let app = App::new(&report);
    let clipboard = NoopClipboard;
    let browser = FakeBrowserOpener::new(Ok(()));
    let ports = ports_with(Some(pr_context()), &clipboard, &browser);

    let actual = open_pr_in_browser(app, &ports);

    assert_eq!(None, actual.status());
    assert_eq!(
        Some("https://github.com/hiro-o918/rinkaku/pull/42".to_string()),
        *browser.opened_url.borrow()
    );
}

#[test]
fn should_set_an_error_status_message_when_the_browser_port_fails() {
    let report = empty_report();
    let app = App::new(&report);
    let clipboard = NoopClipboard;
    let browser = FakeBrowserOpener::new(Err("no display".to_string()));
    let ports = ports_with(Some(pr_context()), &clipboard, &browser);

    let actual = open_pr_in_browser(app, &ports);

    assert_eq!(Some("error opening browser: no display"), actual.status());
}
