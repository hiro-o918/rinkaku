//! Posts a pending GitHub PR review (ADR 0048 sink A) via `gh api`, one
//! call: open the review, attach every comment, and submit with the
//! chosen verdict — matching how a human reviews on github.com and how
//! `fetch_pr_info` already shells out to `gh` for read-only PR data.

use rinkaku_tui::review::ports::ReviewSubmitter;
use rinkaku_tui::review::{PrContext, RenderedComment, Verdict};

/// [`ReviewSubmitter`] backed by `gh api repos/{owner}/{repo}/pulls/{number}/reviews`.
pub(crate) struct GhReviewSubmitter;

impl ReviewSubmitter for GhReviewSubmitter {
    fn submit_review(
        &self,
        ctx: &PrContext,
        verdict: Verdict,
        summary: &str,
        comments: &[RenderedComment],
    ) -> Result<(), String> {
        let body = review_request_json(ctx, verdict, summary, comments);
        let endpoint = format!(
            "repos/{owner}/{repo}/pulls/{number}/reviews",
            owner = ctx.owner,
            repo = ctx.repo,
            number = ctx.number
        );

        let mut command = std::process::Command::new("gh");
        command
            .args(["api", &endpoint, "--method", "POST", "--input", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|err| format!("failed to spawn gh: {err}"))?;

        {
            use std::io::Write;
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| "failed to open gh's stdin".to_string())?;
            stdin
                .write_all(body.to_string().as_bytes())
                .map_err(|err| format!("failed to write request body to gh: {err}"))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|err| format!("failed to wait for gh: {err}"))?;
        if !output.status.success() {
            return Err(format!(
                "gh api {endpoint} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }
}

/// Builds the JSON body `gh api .../reviews --input -` expects: `event`
/// (GitHub's verdict string), `commit_id` (the PR's head commit, so the
/// review anchors to the exact diff the TUI showed), `body` (the fixed
/// review summary), and one `comments` entry per [`RenderedComment`] —
/// `start_line`/`start_side` are omitted when `start_line` is `None`
/// (GitHub's API distinguishes a single-line comment by their absence,
/// rather than by `start_line == line`). Extracted as its own pure
/// function so the request shape is unit-testable without shelling out to
/// `gh`, mirroring `pr_info::parse_pr_view_json`'s own "parse/build
/// separately from the process call" split.
fn review_request_json(
    ctx: &PrContext,
    verdict: Verdict,
    summary: &str,
    comments: &[RenderedComment],
) -> serde_json::Value {
    let comments_json: Vec<serde_json::Value> = comments
        .iter()
        .map(|comment| {
            let mut entry = serde_json::json!({
                "path": comment.path,
                "line": comment.line,
                "side": "RIGHT",
                "body": comment.body,
            });
            if let Some(start_line) = comment.start_line {
                entry["start_line"] = serde_json::json!(start_line);
                entry["start_side"] = serde_json::json!("RIGHT");
            }
            entry
        })
        .collect();

    serde_json::json!({
        "commit_id": ctx.head_sha,
        "event": verdict_event(verdict),
        "body": summary,
        "comments": comments_json,
    })
}

/// GitHub review API's own `event` string per [`Verdict`].
fn verdict_event(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "APPROVE",
        Verdict::RequestChanges => "REQUEST_CHANGES",
        Verdict::Comment => "COMMENT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn pr_context() -> PrContext {
        PrContext {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            number: 42,
            head_sha: "abc123".to_string(),
        }
    }

    #[test]
    fn should_build_request_json_with_single_line_comment() {
        let comments = vec![RenderedComment {
            path: "src/lib.rs".to_string(),
            line: 10,
            start_line: None,
            body: "please rename this".to_string(),
        }];

        let actual = review_request_json(&pr_context(), Verdict::Approve, "Looks good.", &comments);

        assert_eq!(
            serde_json::json!({
                "commit_id": "abc123",
                "event": "APPROVE",
                "body": "Looks good.",
                "comments": [
                    {
                        "path": "src/lib.rs",
                        "line": 10,
                        "side": "RIGHT",
                        "body": "please rename this",
                    }
                ],
            }),
            actual
        );
    }

    #[test]
    fn should_build_request_json_with_multi_line_comment() {
        let comments = vec![RenderedComment {
            path: "src/lib.rs".to_string(),
            line: 18,
            start_line: Some(12),
            body: "this whole block needs a test".to_string(),
        }];

        let actual = review_request_json(
            &pr_context(),
            Verdict::RequestChanges,
            "Needs work.",
            &comments,
        );

        assert_eq!(
            serde_json::json!({
                "commit_id": "abc123",
                "event": "REQUEST_CHANGES",
                "body": "Needs work.",
                "comments": [
                    {
                        "path": "src/lib.rs",
                        "line": 18,
                        "side": "RIGHT",
                        "start_line": 12,
                        "start_side": "RIGHT",
                        "body": "this whole block needs a test",
                    }
                ],
            }),
            actual
        );
    }

    #[test]
    fn should_build_request_json_with_comment_verdict_and_no_comments() {
        let actual = review_request_json(&pr_context(), Verdict::Comment, "Just a note.", &[]);

        assert_eq!(
            serde_json::json!({
                "commit_id": "abc123",
                "event": "COMMENT",
                "body": "Just a note.",
                "comments": [],
            }),
            actual
        );
    }
}
