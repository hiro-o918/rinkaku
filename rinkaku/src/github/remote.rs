//! GitHub remote URL parsing and `git remote get-url origin` lookup.

/// Extracts `(owner, repo)` from a git remote URL, if it points at
/// GitHub. Accepts the forms `git remote get-url` can return for a GitHub
/// remote: `https://github.com/<owner>/<repo>`, the same with a `.git`
/// suffix, the scp-like SSH form `git@github.com:<owner>/<repo>(.git)`,
/// and the explicit `ssh://` form `ssh://git@github.com/<owner>/<repo>
/// (.git)`. Any other host, or a string that doesn't parse as one of
/// these forms, yields `None` — used by `main` to decide whether the
/// current directory's `origin` matches a `--pr` URL's repository (ADR
/// 0005), where "not GitHub" and "malformed" are both simply "no match".
pub(crate) fn parse_github_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let rest = rest.strip_suffix(".git").unwrap_or(rest);

    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    match segments.as_slice() {
        [owner, repo] => Some((owner.to_string(), repo.to_string())),
        _ => None,
    }
}

/// Whether `remote_url` (as returned by `git remote get-url origin`)
/// points at the same GitHub repository as `owner`/`repo`. GitHub
/// owner/repo names are case-insensitive, so the comparison is too — a
/// clone whose `origin` is `.../Octocat/Hello-World` must still be
/// recognized as matching a `--pr` URL spelled `.../octocat/hello-world`.
/// A `remote_url` that isn't a GitHub remote at all (`parse_github_remote`
/// returns `None`) never matches.
pub(crate) fn github_remote_matches(remote_url: &str, owner: &str, repo: &str) -> bool {
    match parse_github_remote(remote_url) {
        Some((remote_owner, remote_repo)) => {
            remote_owner.eq_ignore_ascii_case(owner) && remote_repo.eq_ignore_ascii_case(repo)
        }
        None => false,
    }
}

/// Runs `git remote get-url origin` in `cwd` (or the process's current
/// directory when `None`) and returns its stdout, trimmed. `Ok(None)`
/// (rather than an `Err`) when the command fails — not being inside a git
/// repository, or a repository with no `origin` remote, are both
/// expected, ordinary situations for `--pr` URL mode (ADR 0005): they
/// simply mean "the current directory doesn't match, use the cache"
/// rather than a fatal error worth surfacing to the user.
pub(crate) fn git_remote_origin_url(
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<Option<String>> {
    let mut command = std::process::Command::new("git");
    command.args(["remote", "get-url", "origin"]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(String::from_utf8(output.stdout)?.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{init_repo_with_committed_file, run_git};
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_parse_https_url(
        "https://github.com/octocat/hello-world",
        Some(("octocat".to_string(), "hello-world".to_string()))
    )]
    #[case::should_parse_https_url_with_dot_git_suffix(
        "https://github.com/octocat/hello-world.git",
        Some(("octocat".to_string(), "hello-world".to_string()))
    )]
    #[case::should_parse_scp_like_ssh_url(
        "git@github.com:octocat/hello-world.git",
        Some(("octocat".to_string(), "hello-world".to_string()))
    )]
    #[case::should_parse_scp_like_ssh_url_without_dot_git_suffix(
        "git@github.com:octocat/hello-world",
        Some(("octocat".to_string(), "hello-world".to_string()))
    )]
    #[case::should_parse_explicit_ssh_url(
        "ssh://git@github.com/octocat/hello-world.git",
        Some(("octocat".to_string(), "hello-world".to_string()))
    )]
    #[case::should_parse_explicit_ssh_url_without_dot_git_suffix(
        "ssh://git@github.com/octocat/hello-world",
        Some(("octocat".to_string(), "hello-world".to_string()))
    )]
    #[case::should_trim_surrounding_whitespace(
        " https://github.com/octocat/hello-world.git \n",
        Some(("octocat".to_string(), "hello-world".to_string()))
    )]
    #[case::should_reject_non_github_host("https://gitlab.com/octocat/hello-world.git", None)]
    #[case::should_reject_url_missing_repo_segment("https://github.com/octocat", None)]
    #[case::should_reject_url_with_extra_path_segment(
        "https://github.com/octocat/hello-world/extra",
        None
    )]
    #[case::should_reject_empty_string("", None)]
    fn should_parse_github_remote(#[case] url: &str, #[case] expected: Option<(String, String)>) {
        let actual = parse_github_remote(url);

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_match_identical_owner_and_repo(
        "https://github.com/octocat/hello-world.git",
        "octocat",
        "hello-world",
        true
    )]
    #[case::should_match_case_insensitively(
        "https://github.com/Octocat/Hello-World.git",
        "octocat",
        "hello-world",
        true
    )]
    #[case::should_not_match_different_repo(
        "https://github.com/octocat/hello-world.git",
        "octocat",
        "other-repo",
        false
    )]
    #[case::should_not_match_different_owner(
        "https://github.com/octocat/hello-world.git",
        "someone-else",
        "hello-world",
        false
    )]
    #[case::should_not_match_non_github_remote(
        "https://gitlab.com/octocat/hello-world.git",
        "octocat",
        "hello-world",
        false
    )]
    fn should_check_github_remote_match(
        #[case] remote_url: &str,
        #[case] owner: &str,
        #[case] repo: &str,
        #[case] expected: bool,
    ) {
        let actual = github_remote_matches(remote_url, owner, repo);

        assert_eq!(expected, actual);
    }

    // Integration test for `--pr` URL mode's cwd-vs-cache decision (ADR
    // 0005): a real repository with an `origin` remote set must have that
    // URL surfaced by `git_remote_origin_url` so `github_remote_matches`
    // (already unit-tested above against arbitrary strings) can decide
    // whether to reuse the cwd. Exercises the subprocess wrapper itself
    // rather than `github_remote_matches`'s string logic, which is
    // already covered directly.
    #[test]
    fn should_return_origin_url_when_repository_has_an_origin_remote() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo() {}\n");
        run_git(
            dir.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/octocat/hello-world.git",
            ],
        );

        let actual =
            git_remote_origin_url(Some(dir.path())).expect("git remote get-url should not error");

        assert_eq!(
            Some("https://github.com/octocat/hello-world.git".to_string()),
            actual
        );
    }

    // Sibling case: a repository with no `origin` remote at all (rather
    // than a missing/misconfigured one) must come back as `Ok(None)`, not
    // an `Err` — ADR 0005 treats "doesn't match" and "isn't even a clone"
    // identically as "use the cache", so this must not be a fatal error.
    #[test]
    fn should_return_none_when_repository_has_no_origin_remote() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

        let actual = git_remote_origin_url(Some(dir.path()))
            .expect("missing origin remote should not error");

        assert_eq!(None, actual);
    }

    // Sibling case: a directory that isn't a git repository at all must
    // also come back as `Ok(None)`, matching the "run outside any clone"
    // scenario ADR 0005's cache path exists to handle.
    #[test]
    fn should_return_none_when_directory_is_not_a_git_repository() {
        let dir = tempfile::TempDir::new().expect("create tempdir");

        let actual = git_remote_origin_url(Some(dir.path()))
            .expect("a non-repository directory should not error");

        assert_eq!(None, actual);
    }
}
