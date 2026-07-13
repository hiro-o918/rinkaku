//! ADR 0010 `.gitattributes`-driven generated-path resolution, extracted
//! from `main.rs`.
//!
//! Two shell-out entry points share a NUL-parsing tail: `check_generated_paths`
//! passes paths as CLI args (for a diff's changed set), and
//! `check_generated_paths_batch` streams them via `git check-attr --stdin` for
//! whole-repo indexing (thousands of paths — would risk ARG_MAX as argv).
//! `parse_generated_paths` interprets the NUL-separated `path\0attr\0value`
//! triples that both variants produce.

use std::io::Write;

pub(crate) fn check_generated_paths(
    cwd: Option<&std::path::Path>,
    paths: &[String],
) -> std::collections::HashSet<String> {
    if paths.is_empty() {
        return std::collections::HashSet::new();
    }

    let mut command = std::process::Command::new("git");
    command
        .args(["check-attr", "-z", "diff", "linguist-generated", "--"])
        .args(paths);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let Ok(output) = command.output() else {
        return std::collections::HashSet::new();
    };
    if !output.status.success() {
        return std::collections::HashSet::new();
    }
    let Ok(stdout) = String::from_utf8(output.stdout) else {
        return std::collections::HashSet::new();
    };
    parse_generated_paths(&stdout)
}

pub(crate) fn check_generated_paths_batch(
    cwd: Option<&std::path::Path>,
    paths: &[String],
) -> std::collections::HashSet<String> {
    if paths.is_empty() {
        return std::collections::HashSet::new();
    }

    let mut command = std::process::Command::new("git");
    command
        .args(["check-attr", "--stdin", "-z", "diff", "linguist-generated"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let Ok(mut child) = command.spawn() else {
        return std::collections::HashSet::new();
    };
    let Some(mut stdin) = child.stdin.take() else {
        return std::collections::HashSet::new();
    };
    let Some(mut stdout) = child.stdout.take() else {
        return std::collections::HashSet::new();
    };
    // Drained on its own thread for the same reason
    // `read_git_show_files_batch` drains stderr concurrently: an unread
    // pipe that fills up would otherwise be a second way for this call to
    // deadlock, independent of the stdin/stdout split above.
    let stderr = child.stderr.take();
    let stderr_reader = stderr.map(|mut stderr| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut stderr, &mut buf);
            buf
        })
    });

    let paths_owned: Vec<String> = paths.to_vec();
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        for path in &paths_owned {
            stdin.write_all(path.as_bytes())?;
            stdin.write_all(b"\0")?;
        }
        // Dropping `stdin` here (end of scope) closes the pipe, which is
        // what makes `git check-attr --stdin` stop waiting for more input
        // and start producing output.
        Ok(())
    });

    let mut stdout_bytes = Vec::new();
    if std::io::Read::read_to_end(&mut stdout, &mut stdout_bytes).is_err() {
        return std::collections::HashSet::new();
    }

    // Propagate a stdin-write failure (thread panic, or the write itself
    // returning `Err`) as "nothing resolved" rather than unwrapping —
    // best-effort, same as every other failure mode here.
    let Ok(Ok(())) = writer.join() else {
        return std::collections::HashSet::new();
    };

    let status = child.wait();
    if let Some(reader) = stderr_reader {
        let _ = reader.join();
    }
    let Ok(status) = status else {
        return std::collections::HashSet::new();
    };
    if !status.success() {
        return std::collections::HashSet::new();
    }
    let Ok(stdout_text) = String::from_utf8(stdout_bytes) else {
        return std::collections::HashSet::new();
    };
    parse_generated_paths(&stdout_text)
}

fn parse_generated_paths(output: &str) -> std::collections::HashSet<String> {
    let fields: Vec<&str> = output
        .split('\0')
        .filter(|field| !field.is_empty())
        .collect();

    let mut generated = std::collections::HashSet::new();
    for triple in fields.chunks_exact(3) {
        let [path, attribute, value] = triple else {
            continue;
        };
        let is_generated = (*attribute == "diff" && *value == "unset")
            || (*attribute == "linguist-generated" && matches!(*value, "set" | "true"));
        if is_generated {
            generated.insert((*path).to_string());
        }
    }
    generated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::run_git;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    #[test]
    fn should_mark_path_generated_when_diff_attribute_is_unset() {
        let output = "Cargo.lock\0diff\0unset\0Cargo.lock\0linguist-generated\0unspecified\0";

        let expected: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();
        let actual = parse_generated_paths(output);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_mark_path_generated_when_linguist_generated_attribute_value_is_true() {
        // `.gitattributes` line: `gen/*.go linguist-generated=true` — the
        // common GitHub Linguist spelling. Verified against a real
        // `git check-attr -z` run: this assignment reports the *literal*
        // string `true`, not the boolean attribute value `set` (see
        // `should_mark_path_generated_when_linguist_generated_attribute_is_bare_set`
        // for the other spelling).
        let output = "gen/foo.go\0diff\0unspecified\0gen/foo.go\0linguist-generated\0true\0";

        let expected: HashSet<String> = ["gen/foo.go".to_string()].into_iter().collect();
        let actual = parse_generated_paths(output);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_mark_path_generated_when_linguist_generated_attribute_is_bare_set() {
        // `.gitattributes` line: `gen/*.go linguist-generated` (no
        // `=value`) — reports the boolean attribute value `set`, distinct
        // from the `=true` spelling's literal `true` value (verified
        // against a real `git check-attr -z` run).
        let output = "gen/foo.go\0diff\0unspecified\0gen/foo.go\0linguist-generated\0set\0";

        let expected: HashSet<String> = ["gen/foo.go".to_string()].into_iter().collect();
        let actual = parse_generated_paths(output);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_mark_path_generated_when_both_attributes_are_unspecified() {
        let output = "normal.rs\0diff\0unspecified\0normal.rs\0linguist-generated\0unspecified\0";

        let expected: HashSet<String> = HashSet::new();
        let actual = parse_generated_paths(output);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_mark_only_matching_path_when_multiple_paths_are_queried() {
        let output = "\
Cargo.lock\0diff\0unset\0Cargo.lock\0linguist-generated\0unspecified\0normal.rs\0diff\0unspecified\0normal.rs\0linguist-generated\0unspecified\0";

        let expected: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();
        let actual = parse_generated_paths(output);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_set_when_output_is_empty() {
        let expected: HashSet<String> = HashSet::new();
        let actual = parse_generated_paths("");

        assert_eq!(expected, actual);
    }

    // Integration test for ADR 0010: `check_generated_paths` must shell out
    // to a real `git check-attr` and report exactly the paths a
    // `.gitattributes` file marks `-diff` or `linguist-generated`, leaving
    // an ordinary tracked file out.
    #[test]
    fn should_report_paths_marked_generated_in_gitattributes() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        std::fs::write(
            dir.path().join(".gitattributes"),
            "Cargo.lock -diff\ngen/*.go linguist-generated=true\n",
        )
        .expect("write .gitattributes");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");
        std::fs::write(dir.path().join("normal.rs"), "").expect("write normal.rs");
        std::fs::create_dir_all(dir.path().join("gen")).expect("create gen dir");
        std::fs::write(dir.path().join("gen/foo.go"), "").expect("write gen/foo.go");

        let paths = vec![
            "Cargo.lock".to_string(),
            "normal.rs".to_string(),
            "gen/foo.go".to_string(),
        ];
        let actual = check_generated_paths(Some(dir.path()), &paths);

        let expected: HashSet<String> = ["Cargo.lock".to_string(), "gen/foo.go".to_string()]
            .into_iter()
            .collect();
        assert_eq!(expected, actual);
    }

    // Best-effort contract (ADR 0010): a directory that is not a git
    // repository at all must not turn attribute resolution into a hard
    // error — the caller degrades to "nothing is generated" instead.
    #[test]
    fn should_return_empty_set_when_cwd_is_not_a_git_repository() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");

        let actual = check_generated_paths(Some(dir.path()), &["Cargo.lock".to_string()]);

        let expected: HashSet<String> = HashSet::new();
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_set_when_paths_is_empty() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);

        let actual = check_generated_paths(Some(dir.path()), &[]);

        let expected: HashSet<String> = HashSet::new();
        assert_eq!(expected, actual);
    }

    // check_generated_paths_batch is check_generated_paths's sibling for
    // TagsResolver's repo-wide index: the same git check-attr resolution,
    // but paths are streamed via --stdin -z instead of passed as CLI
    // arguments, since the index covers every git-ls-files-tracked path
    // (potentially thousands) rather than just a diff's changed files —
    // large enough to risk ARG_MAX if passed as argv.
    #[test]
    fn should_report_paths_marked_generated_via_stdin_batch() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        std::fs::write(
            dir.path().join(".gitattributes"),
            "Cargo.lock -diff\ngen/*.go linguist-generated=true\n",
        )
        .expect("write .gitattributes");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");
        std::fs::write(dir.path().join("normal.rs"), "").expect("write normal.rs");
        std::fs::create_dir_all(dir.path().join("gen")).expect("create gen dir");
        std::fs::write(dir.path().join("gen/foo.go"), "").expect("write gen/foo.go");

        let paths = vec![
            "Cargo.lock".to_string(),
            "normal.rs".to_string(),
            "gen/foo.go".to_string(),
        ];
        let actual = check_generated_paths_batch(Some(dir.path()), &paths);

        let expected: HashSet<String> = ["Cargo.lock".to_string(), "gen/foo.go".to_string()]
            .into_iter()
            .collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_set_when_batch_cwd_is_not_a_git_repository() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");

        let actual = check_generated_paths_batch(Some(dir.path()), &["Cargo.lock".to_string()]);

        let expected: HashSet<String> = HashSet::new();
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_set_when_batch_paths_is_empty() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);

        let actual = check_generated_paths_batch(Some(dir.path()), &[]);

        let expected: HashSet<String> = HashSet::new();
        assert_eq!(expected, actual);
    }

    // Regression case: a large path count must not exceed argv limits
    // since paths are streamed via stdin, not passed as CLI arguments —
    // this pins down that the batch path actually works at a scale that
    // would risk ARG_MAX if passed as argv (check_generated_paths's own
    // approach), not just that it works for a handful of paths.
    #[test]
    fn should_handle_many_paths_via_stdin_without_hitting_arg_limits() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        std::fs::write(
            dir.path().join(".gitattributes"),
            "gen/*.go linguist-generated=true\n",
        )
        .expect("write .gitattributes");
        std::fs::create_dir_all(dir.path().join("gen")).expect("create gen dir");

        let mut paths = Vec::new();
        for i in 0..5000 {
            let path = format!("gen/file{i}.go");
            std::fs::write(dir.path().join(&path), "").expect("write generated file");
            paths.push(path);
        }

        let actual = check_generated_paths_batch(Some(dir.path()), &paths);

        assert_eq!(paths.len(), actual.len());
    }
}
