//! Per-file readers used by `pipeline`: read from the working tree
//! (stdin mode) or from `git show <head>:<path>` (`--base`/`--pr` modes,
//! for parity with the commit the diff was generated against).

/// Reads a changed file's new-side content off the working tree.
pub(crate) fn read_working_tree_file(path: &str) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

/// Reads a changed file's content as committed at `head`, via
/// `git show <head>:<path>`. Used in `--base` mode so the content read
/// always matches the commit the diff was generated against, independent
/// of the working tree's current state.
///
/// `cwd` selects the repository to run `git` in; `None` uses the process's
/// current directory (production callers), `Some(dir)` pins it to a
/// specific directory (tests, so they don't depend on or mutate the
/// process-wide current directory).
pub(crate) fn read_git_show_file(
    cwd: Option<&std::path::Path>,
    head: &str,
    path: &str,
) -> std::io::Result<String> {
    let object = format!("{head}:{path}");
    let mut command = std::process::Command::new("git");
    command.args(["show", &object]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "git show {object} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::init_repo_with_committed_file;
    use pretty_assertions::assert_eq;
    // Integration test for the must-fix design: `--base` mode must read
    // file content via `git show <head>:<path>`, not off the working tree.
    // A dirty working tree (uncommitted edit) must not affect what gets
    // read — only the committed content at `head` should come back.
    #[test]
    fn should_read_committed_content_when_working_tree_is_dirty() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let committed = "fn foo(a: i32) -> i32 {\n    a\n}\n";
        init_repo_with_committed_file(dir.path(), committed);

        // Dirty the working tree after the commit: if `read_git_show_file`
        // fell back to the working tree, it would read this instead.
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn foo(a: i32) -> i32 {\n    a + 999\n}\n",
        )
        .expect("dirty the working tree");

        let actual = read_git_show_file(Some(dir.path()), "HEAD", "src/lib.rs")
            .expect("git show should succeed for a committed file");

        assert_eq!(committed, actual);
    }
}
