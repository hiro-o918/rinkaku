//! `gh pr view` info fetch: the `PrInfo` shape and its JSON parser.

/// The subset of `gh pr view --json number,baseRefName,baseRefOid,
/// headRefOid` this binary needs to drive `--pr` mode (ADR 0004, ADR
/// 0007): which PR, what its base branch is called (fallback path),
/// the commit its base was pinned to at PR time (`base_ref_oid`,
/// preferred — see ADR 0007), and the exact commit its head is expected
/// to be at (checked against what `git fetch` actually retrieves, see
/// `main`'s mismatch check).
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct PrInfo {
    pub(crate) number: u64,
    #[serde(rename = "baseRefName")]
    pub(crate) base_ref_name: String,
    #[serde(rename = "baseRefOid")]
    pub(crate) base_ref_oid: String,
    #[serde(rename = "headRefOid")]
    pub(crate) head_ref_oid: String,
}
/// Parses `gh pr view --json number,baseRefName,baseRefOid,headRefOid`'s
/// stdout. Split out from `fetch_pr_info` so the JSON shape can be
/// unit-tested without shelling out to `gh`.
pub(crate) fn parse_pr_view_json(json: &str) -> anyhow::Result<PrInfo> {
    Ok(serde_json::from_str(json)?)
}

/// Checks that the head commit `git fetch refs/pull/<number>/head`
/// actually retrieved (`fetched_head`) is the one `gh pr view` reported
/// (`reported_head`). A mismatch usually means the PR belongs to a
/// different repository than the target clone's `origin` remote (see
/// [`fetch_pr_info`]'s doc comment on why `gh` and `git` are deliberately
/// allowed to disagree so this check can catch it), or that the PR was
/// updated between resolving and fetching. Extracted from `main` as a
/// pure function so the check is unit-testable without `gh`/`git`.
pub(crate) fn ensure_fetched_head_matches(
    number: u64,
    fetched_head: &str,
    reported_head: &str,
) -> anyhow::Result<()> {
    if fetched_head == reported_head {
        return Ok(());
    }
    anyhow::bail!(
        "fetched PR #{number} head ({fetched_head}) does not match `gh`'s reported head \
         ({reported_head}); this usually means the PR belongs to a different repository than \
         the target clone's `origin` remote, or the PR was updated between resolving it \
         and fetching it — verify `origin` points at the PR's repository and re-run",
    );
}

/// Runs `gh pr view <arg> --json number,baseRefName,baseRefOid,headRefOid`
/// and parses the result.
///
/// Takes the user's original `--pr` argument (URL or bare number) rather
/// than the number `parse_pr_arg` extracts from it, and this is load-bearing
/// rather than cosmetic: `gh pr view <number>` always resolves against the
/// *current directory's* repository, ignoring any owner/repo encoded in a
/// URL the user passed. If it were fed only the number, `--pr
/// https://github.com/other/repo/pull/5` run inside an unrelated clone
/// would silently resolve and analyze that clone's own PR #5. Passing the
/// full URL through lets `gh` itself resolve against the URL's repository,
/// so a foreign-repo URL makes `gh` report a `headRefOid` that the
/// cwd-scoped `git fetch origin refs/pull/<n>/head` in `main` cannot
/// possibly match — the mismatch check there is what actually surfaces the
/// error, and it only works if `gh` and `git` are allowed to disagree on
/// which repository they resolved against.
pub(crate) fn fetch_pr_info(arg: &str) -> anyhow::Result<PrInfo> {
    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "view",
            arg,
            "--json",
            "number,baseRefName,baseRefOid,headRefOid",
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "gh pr view {arg} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    parse_pr_view_json(&String::from_utf8(output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_parse_pr_view_json_into_pr_info() {
        let json = r#"{"number":123,"baseRefName":"main","baseRefOid":"base789","headRefOid":"abc123def456"}"#;

        let actual = parse_pr_view_json(json).expect("expected valid JSON to parse");

        assert_eq!(
            PrInfo {
                number: 123,
                base_ref_name: "main".to_string(),
                base_ref_oid: "base789".to_string(),
                head_ref_oid: "abc123def456".to_string(),
            },
            actual
        );
    }

    #[test]
    fn should_fail_to_parse_pr_view_json_when_a_required_field_is_missing() {
        let json = r#"{"number":123,"baseRefName":"main"}"#;

        let actual = parse_pr_view_json(json);

        assert!(actual.is_err());
    }

    #[test]
    fn should_accept_fetched_head_when_it_matches_the_reported_head() {
        let actual = ensure_fetched_head_matches(76, "abc123", "abc123");

        assert!(actual.is_ok());
    }

    #[test]
    fn should_reject_fetched_head_with_a_repository_hint_when_it_differs_from_the_reported_head() {
        let actual = ensure_fetched_head_matches(76, "abc123", "def456");

        let message = actual.expect_err("expected a mismatch error").to_string();
        assert_eq!(
            "fetched PR #76 head (abc123) does not match `gh`'s reported head (def456); this \
             usually means the PR belongs to a different repository than the target clone's \
             `origin` remote, or the PR was updated between resolving it and fetching it — \
             verify `origin` points at the PR's repository and re-run",
            message
        );
    }
}
