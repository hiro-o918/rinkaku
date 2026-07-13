//! Local git subprocess wrappers used by the composition root and other
//! modules: `git diff`, `git ls-files`, and `git rev-parse
//! --show-toplevel`.

pub(crate) fn run_git_diff(
    base: &str,
    head: &str,
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<String> {
    let range = format!("{base}...{head}");
    let mut command = std::process::Command::new("git");
    command.args(["diff", &range]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git diff {range} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub(crate) fn list_git_files(cwd: Option<&std::path::Path>) -> anyhow::Result<Vec<String>> {
    let mut command = std::process::Command::new("git");
    command.args(["ls-files"]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Lists tracked files for ADR 0017's whole-repo outline, same as
/// `list_git_files(cwd)`, but with guidance attached to a failure via
/// `anyhow::Context`: bare `rinkaku` (this mode's default, the first thing
/// a new user is likely to try) run outside a git repository would
/// otherwise surface only `list_git_files`'s raw `git ls-files` stderr
/// (e.g. "fatal: not a git repository ..."), which does not tell the reader
/// what rinkaku itself expects instead. Kept as its own function (rather
/// than adding this message inside `list_git_files` itself) since that
/// function's error is reused as-is by every other caller (`--base`/`--pr`'s
/// own indexing pass in `build_resolver`) where this specific guidance
/// would not apply.
pub(crate) fn list_repo_files_for_outline(
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    list_git_files(cwd).context(
        "run rinkaku inside a git repository, or pipe a diff (e.g. `gh pr diff 123 | rinkaku`) \
         or pass --base <ref>",
    )
}

pub(crate) fn resolve_repo_root(cwd: Option<&std::path::Path>) -> std::path::PathBuf {
    let mut command = std::process::Command::new("git");
    command.args(["rev-parse", "--show-toplevel"]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let toplevel = command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| std::path::PathBuf::from(stdout.trim()));

    toplevel.unwrap_or_else(|| match cwd {
        Some(cwd) => cwd.to_path_buf(),
        None => std::env::current_dir().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::init_repo_with_committed_file;
    use pretty_assertions::assert_eq;
    // Regression test for the TUI source view failing whenever `rinkaku`
    // is launched from a subdirectory of the repository (the bug this
    // function exists to fix): `git rev-parse --show-toplevel` run from
    // `src/` must still resolve to the repository root, not `src/` itself
    // — `resolve_repo_root`'s own doc comment explains why `Report` paths
    // need the *root*, not the process's actual current directory, to
    // join against.
    #[test]
    fn should_resolve_repository_root_when_cwd_is_a_subdirectory() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo() {}\n");
        let subdir = dir.path().join("src");

        let actual = resolve_repo_root(Some(&subdir));

        // Compare canonicalized paths on both sides: `git rev-parse
        // --show-toplevel`'s output and `tempfile::TempDir::path()` can
        // differ by a symlink resolution (e.g. macOS's `/tmp` ->
        // `/private/tmp`), which is not the thing this test is checking.
        let expected = dir.path().canonicalize().expect("canonicalize expected");
        let actual = actual.canonicalize().expect("canonicalize actual");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_fall_back_to_cwd_when_directory_is_not_a_git_repository() {
        let dir = tempfile::TempDir::new().expect("create tempdir");

        let actual = resolve_repo_root(Some(dir.path()));

        assert_eq!(dir.path(), actual);
    }

    mod list_repo_files_for_outline_tests {
        use super::*;
        use pretty_assertions::assert_eq;

        // Regression test for the unfriendly-error fix: running whole-repo
        // mode outside a git repository must not surface only
        // `list_git_files`'s raw `git ls-files` stderr — the wrapped
        // message must guide the reader toward what rinkaku actually
        // expects (a git repo, a piped diff, or `--base`).
        #[test]
        fn should_include_guidance_in_error_when_cwd_is_not_a_git_repository() {
            let dir = tempfile::TempDir::new().expect("create tempdir");

            let actual = list_repo_files_for_outline(Some(dir.path()));

            let error = actual.expect_err("a non-git directory must fail");
            let message = format!("{error:#}");
            assert!(
                message.contains("run rinkaku inside a git repository"),
                "error message did not contain the expected guidance: {message}"
            );
        }

        #[test]
        fn should_return_tracked_paths_when_cwd_is_a_git_repository() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

            let actual = list_repo_files_for_outline(Some(dir.path()))
                .expect("a git repository must succeed");

            assert_eq!(vec!["src/lib.rs".to_string()], actual);
        }
    }
}
