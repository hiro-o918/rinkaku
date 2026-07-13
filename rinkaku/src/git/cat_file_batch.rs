//! Bulk file reader used by `pipeline::build_resolver` to fetch every
//! tracked file at `head` via a single long-lived `git cat-file --batch`
//! subprocess, instead of one `git show` spawn per file.

use std::io::{BufRead, Read, Write};

/// Reads every path in `paths` at `head` (`git show <head>:<path>`'s
/// content, for each path) via a single long-lived `git cat-file --batch`
/// child process, instead of spawning one `git show` subprocess per path
/// (`read_git_show_file`'s approach — fine for the handful of files a diff
/// actually changes, but prohibitively slow for `build_resolver`'s
/// repository-wide index, which reads every tracked file: a repository
/// with a few thousand tracked files previously meant a few thousand
/// process spawns just to build the dependency index).
///
/// Protocol: `git cat-file --batch` reads `<object>\n` requests from
/// stdin and writes a response to stdout per request, documented in
/// `git help cat-file`'s BATCH OUTPUT section — mainly the "found" shape
/// (`<oid> <type> <size>\n` followed by exactly `size` content bytes and
/// a trailing `\n`) and several single-line, no-content shapes
/// (`<object> missing`, `<object> ambiguous`, `<oid> submodule`, ...);
/// see `read_cat_file_batch_response`'s doc comment for exactly which
/// shapes are treated as "skip this path" versus a hard failure.
///
/// Requests and responses are sent one at a time (write a request, then
/// immediately read its response) rather than writing every request up
/// front: `git cat-file --batch`'s stdout is a pipe with a bounded OS
/// buffer, and with ~thousands of paths the parent could deadlock writing
/// requests while the child blocks trying to write responses into an
/// already-full pipe that nobody is draining. One-at-a-time interleaving
/// avoids that entirely while still cutting the process count from one
/// per file to exactly one for the whole index — the actual cost this
/// change targets. stderr is drained concurrently on a dedicated thread
/// for the same reason (see the inline comment where it's spawned) — a
/// verbose enough diagnostic on stderr could otherwise fill that pipe too
/// and deadlock the same way.
pub(crate) fn read_git_show_files_batch(
    cwd: Option<&std::path::Path>,
    head: &str,
    paths: Vec<String>,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut command = std::process::Command::new("git");
    command
        .args(["cat-file", "--batch"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let mut child = command
        .spawn()
        .map_err(|source| anyhow::anyhow!("failed to start git cat-file --batch: {source}"))?;
    let mut stdin = child
        .stdin
        .take()
        .expect("stdin is piped, so it must be present");
    let stdout = child
        .stdout
        .take()
        .expect("stdout is piped, so it must be present");
    let stderr = child
        .stderr
        .take()
        .expect("stderr is piped, so it must be present");
    let mut reader = std::io::BufReader::new(stdout);

    // Drained on a dedicated thread rather than read after `wait()`: this
    // call writes/reads stdin and stdout on the main thread in lockstep,
    // so nothing here would otherwise ever read stderr. If `git` writes
    // enough diagnostics to fill the OS pipe buffer, the child would block
    // writing to stderr while this thread blocks reading stdout — an
    // indefinite mutual stall neither side can break out of. A concurrent
    // reader keeps that pipe draining regardless of what the main thread
    // is doing.
    let stderr_reader = std::thread::spawn(move || {
        let mut stderr = stderr;
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });

    // The pump's `Result` is captured rather than propagated with `?` here:
    // if the child has already exited (e.g. `cwd` is not a git repository),
    // the very first `writeln!`/`flush` below can fail with a broken-pipe
    // error before this thread ever gets to read the child's stderr. An
    // early return at that point would surface the generic broken-pipe
    // message and lose the actual `git` diagnostic. Running the pump to
    // completion (successful or not) and only then reaping the child keeps
    // the drop-stdin-then-wait-then-join sequence unconditional, so stderr
    // is always drained and available to fold into the final error.
    let pump_result = pump_cat_file_batch_requests(&mut stdin, &mut reader, head, paths);

    // Dropping `stdin` here (end of scope) closes the pipe, which is what
    // makes `git cat-file --batch` exit; `wait()` then just reaps it.
    drop(stdin);
    let status = child
        .wait()
        .map_err(|source| anyhow::anyhow!("failed to wait on git cat-file --batch: {source}"))?;
    // The child has exited, so its stderr end is closed and this join
    // cannot block indefinitely waiting for more output.
    let stderr_output = stderr_reader
        .join()
        .unwrap_or_else(|_| b"<failed to read stderr: reader thread panicked>".to_vec());

    combine_cat_file_batch_result(pump_result, &status, &stderr_output)
}

/// Writes one `<object>\n` request per path and reads its response in
/// lockstep (see `read_git_show_files_batch`'s doc comment for why writes
/// and reads are interleaved rather than batched). Split out from
/// `read_git_show_files_batch` so a write/flush/read failure partway
/// through can be captured as a plain `Result` instead of using `?` to
/// return early past the caller's reaping of the child process.
fn pump_cat_file_batch_requests(
    stdin: &mut std::process::ChildStdin,
    reader: &mut impl BufRead,
    head: &str,
    paths: Vec<String>,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let object = format!("{head}:{path}");
        writeln!(stdin, "{object}").map_err(|source| {
            anyhow::anyhow!("failed to write to git cat-file --batch: {source}")
        })?;
        stdin.flush().map_err(|source| {
            anyhow::anyhow!("failed to flush git cat-file --batch stdin: {source}")
        })?;

        match read_cat_file_batch_response(reader, &object)? {
            Some(content) => files.push((path, content)),
            None => continue,
        }
    }
    Ok(files)
}

/// Combines the request/response pump's outcome with the child's exit
/// status and drained stderr into the final `Result`, once both are known.
///
/// This is a pure decision, split out from `read_git_show_files_batch` so
/// each combination is directly unit-testable without spawning `git`:
///
/// - pump succeeded, exit succeeded → the pump's files, as-is.
/// - pump succeeded, exit failed → the exit-status/stderr error (the pump
///   getting to the end doesn't mean the child considered itself successful).
/// - pump failed, exit failed → the exit-status/stderr error, with the
///   pump's own error appended for extra detail — the stderr text stays the
///   *primary* message (what `.to_string()` returns starts with it) since
///   it is the actionable diagnostic (e.g. `fatal: not a git repository`),
///   not the secondary broken-pipe symptom. This is the EPIPE race: when
///   the child has already exited before the parent's first write, the
///   write fails with a broken-pipe error that on its own says nothing
///   about *why* the child was already gone — observed to consistently win
///   the race on CI, where the generic broken-pipe message otherwise
///   replaced the real `git` diagnostic.
/// - pump failed, exit succeeded → the pump's error, unchanged: the child
///   reported success, so whatever went wrong is on the parent's side (or
///   is otherwise not explained by the child's stderr).
fn combine_cat_file_batch_result(
    pump_result: anyhow::Result<Vec<(String, String)>>,
    status: &std::process::ExitStatus,
    stderr_output: &[u8],
) -> anyhow::Result<Vec<(String, String)>> {
    match (pump_result, status.success()) {
        (Ok(files), true) => Ok(files),
        (Ok(_), false) => Err(anyhow::anyhow!(
            "git cat-file --batch exited with {status}: {}",
            String::from_utf8_lossy(stderr_output)
        )),
        (Err(pump_error), false) => Err(anyhow::anyhow!(
            "git cat-file --batch exited with {status}: {} (pump error: {pump_error})",
            String::from_utf8_lossy(stderr_output)
        )),
        (Err(pump_error), true) => Err(pump_error),
    }
}

/// Reads and parses one `git cat-file --batch` response for `object`
/// (`<head>:<path>`). See `read_git_show_files_batch`'s doc comment for
/// the "found" response shape.
///
/// `git cat-file --batch` has more single-line, no-content-body response
/// shapes than just `<object> missing` — `git help cat-file`'s BATCH
/// OUTPUT section also documents `<object> ambiguous` (an ambiguous short
/// name — not reachable through this call's `<head>:<path>` requests,
/// which are never short/ambiguous, but defended against anyway) and
/// `<oid> submodule` (a gitlink entry whose target commit isn't present
/// in the repository). Any header line that isn't the `<oid> <type>
/// <size>` "found" shape is therefore treated the same way as `missing`:
/// skip this single path, since a single line was already fully consumed
/// by `read_line` and the stream position is well-defined regardless of
/// what that line actually said — there is nothing to desync.
///
/// `Ok(None)` for any such skippable single-line response, or for
/// found-but-non-UTF-8 content (both "skip this path", matching the
/// working-tree read path's `.ok()` handling and restoring the same
/// per-file isolation `read_git_show_file` had before batching). `Err`
/// only for an IO failure reading the header line, the exact-size content
/// bytes, or the trailing newline after a "found" header — those are the
/// only points where the stream's position becomes genuinely unknown
/// (a partial read of `size` content bytes, in particular, means there is
/// no way to know where the next response begins), so recovery for later
/// paths in the same batch is not possible and the whole call must fail.
fn read_cat_file_batch_response(
    reader: &mut impl BufRead,
    object: &str,
) -> anyhow::Result<Option<String>> {
    let mut header = String::new();
    reader.read_line(&mut header).map_err(|source| {
        anyhow::anyhow!("failed to read git cat-file --batch header: {source}")
    })?;
    let header = header.trim_end_matches('\n');

    // "Found" shape: "<oid> <type> <size>", the size being the last
    // whitespace-separated token. Anything else (missing, ambiguous,
    // submodule, or any other single-line shape this code doesn't
    // specifically know about) is a skip, not a hard error — see the doc
    // comment above for why that's safe.
    let Some(size) = header
        .rsplit(' ')
        .next()
        .and_then(|s| s.parse::<usize>().ok())
    else {
        return Ok(None);
    };

    let mut content = vec![0u8; size];
    reader.read_exact(&mut content).map_err(|source| {
        anyhow::anyhow!("failed to read git cat-file --batch content for {object}: {source}")
    })?;
    // Every found response is followed by exactly one trailing newline
    // after the content bytes, regardless of whether the content itself
    // ends in one.
    let mut trailing_newline = [0u8; 1];
    reader.read_exact(&mut trailing_newline).map_err(|source| {
        anyhow::anyhow!(
            "failed to read git cat-file --batch trailing newline for {object}: {source}"
        )
    })?;

    match String::from_utf8(content) {
        Ok(content) => Ok(Some(content)),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{init_repo_with_committed_file, run_git};
    use pretty_assertions::assert_eq;
    // Integration test for the perf fix: a single `git cat-file --batch`
    // process must return the same content `read_git_show_file` would
    // have returned per-file, for every tracked path in one pass.
    #[test]
    fn should_read_every_path_via_a_single_cat_file_batch_process() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").expect("write a.rs");
        std::fs::write(dir.path().join("b.rs"), "fn b() {}\n").expect("write b.rs");
        run_git(dir.path(), &["add", "a.rs", "b.rs"]);
        run_git(dir.path(), &["commit", "-m", "initial commit"]);

        let mut actual = read_git_show_files_batch(
            Some(dir.path()),
            "HEAD",
            vec!["a.rs".to_string(), "b.rs".to_string()],
        )
        .expect("git cat-file --batch should succeed for tracked files");
        actual.sort();

        assert_eq!(
            vec![
                ("a.rs".to_string(), "fn a() {}\n".to_string()),
                ("b.rs".to_string(), "fn b() {}\n".to_string()),
            ],
            actual
        );
    }

    // Sibling case: a dirty working tree must not affect what the batch
    // read returns, same guarantee `read_git_show_file` already provides
    // per-file (`should_read_committed_content_when_working_tree_is_dirty`
    // above).
    #[test]
    fn should_read_committed_content_via_batch_when_working_tree_is_dirty() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let committed = "fn foo(a: i32) -> i32 {\n    a\n}\n";
        init_repo_with_committed_file(dir.path(), committed);

        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn foo(a: i32) -> i32 {\n    a + 999\n}\n",
        )
        .expect("dirty the working tree");

        let actual =
            read_git_show_files_batch(Some(dir.path()), "HEAD", vec!["src/lib.rs".to_string()])
                .expect("git cat-file --batch should succeed for a committed file");

        assert_eq!(
            vec![("src/lib.rs".to_string(), committed.to_string())],
            actual
        );
    }

    // A path `git ls-files` lists but that doesn't resolve to a blob at
    // `head` (e.g. a submodule gitlink entry, or here simply a path that
    // was never committed) must be skipped rather than failing the whole
    // batch — matching `build_resolver`'s existing best-effort handling of
    // per-file read failures.
    #[test]
    fn should_skip_missing_paths_when_reading_via_batch() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

        let actual = read_git_show_files_batch(
            Some(dir.path()),
            "HEAD",
            vec!["src/lib.rs".to_string(), "does/not/exist.rs".to_string()],
        )
        .expect("git cat-file --batch should succeed even with a missing path");

        assert_eq!(
            vec![("src/lib.rs".to_string(), "fn foo() {}\n".to_string())],
            actual
        );
    }

    // A tracked file whose committed content isn't valid UTF-8 (a binary
    // file) must be skipped rather than failing the batch or the whole
    // resolver build — matching `build_resolver`'s existing best-effort
    // handling (content.ok() drops read failures, and a `String::from_utf8`
    // failure is exactly the same kind of "can't use this as source text"
    // situation).
    #[test]
    fn should_skip_non_utf8_content_when_reading_via_batch() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("text.rs"), "fn ok() {}\n").expect("write text.rs");
        std::fs::write(dir.path().join("binary.dat"), [0xff_u8, 0xfe, 0x00, 0x01])
            .expect("write binary.dat");
        run_git(dir.path(), &["add", "text.rs", "binary.dat"]);
        run_git(dir.path(), &["commit", "-m", "initial commit"]);

        let mut actual = read_git_show_files_batch(
            Some(dir.path()),
            "HEAD",
            vec!["text.rs".to_string(), "binary.dat".to_string()],
        )
        .expect("git cat-file --batch should succeed even with binary content present");
        actual.sort();

        assert_eq!(
            vec![("text.rs".to_string(), "fn ok() {}\n".to_string())],
            actual
        );
    }

    mod read_cat_file_batch_response_tests {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_return_content_when_response_is_found() {
            let mut reader = std::io::Cursor::new(b"abc123 blob 5\nhello\n".to_vec());

            let actual = read_cat_file_batch_response(&mut reader, "HEAD:a.rs")
                .expect("a well-formed found response must parse");

            assert_eq!(Some("hello".to_string()), actual);
        }

        #[test]
        fn should_return_none_when_response_is_missing() {
            let mut reader = std::io::Cursor::new(b"HEAD:a.rs missing\n".to_vec());

            let actual = read_cat_file_batch_response(&mut reader, "HEAD:a.rs")
                .expect("a missing response must not be a hard error");

            assert_eq!(None, actual);
        }

        // `git help cat-file`'s BATCH OUTPUT section documents this shape
        // for an ambiguous short name. Not reachable in practice through
        // this codebase's `<head>:<path>` requests (never a short/
        // ambiguous name by construction), but must still be treated as a
        // skip rather than a hard error if git ever emitted it — it's a
        // single line with no content body, so there is nothing to
        // desync on.
        #[test]
        fn should_return_none_when_response_is_ambiguous() {
            let mut reader = std::io::Cursor::new(b"abc1 ambiguous\n".to_vec());

            let actual = read_cat_file_batch_response(&mut reader, "abc1")
                .expect("an ambiguous response must not be a hard error");

            assert_eq!(None, actual);
        }

        // `git help cat-file`'s BATCH OUTPUT section documents this shape
        // for a gitlink (submodule) entry whose target commit isn't
        // present in the repository — exactly the kind of single-file
        // condition the regression this test guards against used to turn
        // into a hard failure for the whole batch (the "<size>" parse
        // used to require the header be an exact `missing` match or
        // parse as `<oid> <type> <size>`; anything else, including this
        // shape, fell into an `Err`).
        #[test]
        fn should_return_none_when_response_is_a_submodule_entry() {
            let mut reader = std::io::Cursor::new(
                b"3eb8e680cc28d03641be1d2af8e098e8ac6a42f8 submodule\n".to_vec(),
            );

            let actual = read_cat_file_batch_response(&mut reader, "HEAD:sub")
                .expect("a submodule response must not be a hard error");

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_found_content_is_not_valid_utf8() {
            let mut reader = std::io::Cursor::new(
                [
                    b"abc123 blob 4\n".as_slice(),
                    &[0xff, 0xfe, 0x00, 0x01],
                    b"\n",
                ]
                .concat(),
            );

            let actual = read_cat_file_batch_response(&mut reader, "HEAD:binary.dat")
                .expect("non-UTF-8 content must not be a hard error");

            assert_eq!(None, actual);
        }

        // Regression guard for the opposite direction of the fix above:
        // a genuine stream desync (here, content truncated shorter than
        // the declared size) must still be a hard error — it cannot be
        // isolated to a single path, unlike the skippable shapes above.
        #[test]
        fn should_return_error_when_content_is_truncated() {
            let mut reader = std::io::Cursor::new(b"abc123 blob 100\nshort\n".to_vec());

            let actual = read_cat_file_batch_response(&mut reader, "HEAD:a.rs");

            assert!(actual.is_err());
        }
    }

    mod combine_cat_file_batch_result_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use std::os::unix::process::ExitStatusExt;

        fn exit_status(code: i32) -> std::process::ExitStatus {
            std::process::ExitStatus::from_raw(code)
        }

        #[test]
        fn should_return_files_when_pump_succeeds_and_status_succeeds() {
            let pump_result = Ok(vec![("a.rs".to_string(), "fn a() {}\n".to_string())]);

            let actual = combine_cat_file_batch_result(pump_result, &exit_status(0), b"")
                .expect("a successful pump and exit status must not error");

            assert_eq!(
                vec![("a.rs".to_string(), "fn a() {}\n".to_string())],
                actual
            );
        }

        #[test]
        fn should_return_stderr_error_when_pump_succeeds_and_status_fails() {
            let pump_result = Ok(vec![]);

            let actual = combine_cat_file_batch_result(
                pump_result,
                &exit_status(1 << 8),
                b"fatal: not a git repository (or any of the parent directories): .git\n",
            );

            let message = actual
                .expect_err("a failing exit status must be an error")
                .to_string();
            assert!(
                message.contains(
                    "fatal: not a git repository (or any of the parent directories): .git"
                ),
                "expected the stderr text in the error message, got: {message:?}"
            );
        }

        // The EPIPE race this whole fix targets: the pump fails first
        // (broken pipe, because the child already exited) and the exit
        // status is also a failure. The stderr diagnostic must still win
        // as the primary message, with the pump's broken-pipe error folded
        // in as extra detail rather than replacing it.
        #[test]
        fn should_prefer_stderr_error_over_pump_error_when_both_fail() {
            let pump_result: anyhow::Result<Vec<(String, String)>> = Err(anyhow::anyhow!(
                "failed to write to git cat-file --batch: Broken pipe (os error 32)"
            ));

            let actual = combine_cat_file_batch_result(
                pump_result,
                &exit_status(128 << 8),
                b"fatal: not a git repository (or any of the parent directories): .git\n",
            );

            let message = actual
                .expect_err("a failing pump and a failing exit status must be an error")
                .to_string();
            assert!(
                message.starts_with("git cat-file --batch exited with"),
                "expected the stderr-derived message to be primary, got: {message:?}"
            );
            assert!(
                message.contains(
                    "fatal: not a git repository (or any of the parent directories): .git"
                ),
                "expected the stderr text in the error message, got: {message:?}"
            );
            assert!(
                message.contains("Broken pipe"),
                "expected the pump error to be folded in as extra detail, got: {message:?}"
            );
        }

        #[test]
        fn should_return_pump_error_unchanged_when_pump_fails_and_status_succeeds() {
            let pump_result: anyhow::Result<Vec<(String, String)>> = Err(anyhow::anyhow!(
                "failed to read git cat-file --batch header: unexpected EOF"
            ));

            let actual = combine_cat_file_batch_result(pump_result, &exit_status(0), b"");

            let message = actual
                .expect_err("a failing pump must be an error even if the exit status succeeded")
                .to_string();
            assert_eq!(
                "failed to read git cat-file --batch header: unexpected EOF",
                message
            );
        }
    }

    // Regression test for the must-fix cleanup: the exit-status error
    // message must include the child's stderr, matching every other
    // subprocess call in this file. Also exercises the concurrent
    // stderr-draining thread end to end (rather than only reasoning about
    // it): a non-git `cwd` makes `git cat-file --batch` write a `fatal:
    // not a git repository...` diagnostic to stderr and exit non-zero,
    // and that diagnostic must show up in the returned error.
    #[test]
    fn should_include_stderr_in_error_when_git_cat_file_batch_exits_non_zero() {
        let dir = tempfile::TempDir::new().expect("create tempdir");

        let actual = read_git_show_files_batch(Some(dir.path()), "HEAD", vec!["a.rs".to_string()]);

        let error = actual.expect_err("a non-git cwd must fail rather than silently succeed");
        let message = error.to_string();
        assert!(
            message.contains("not a git repository"),
            "expected the child's stderr to be included in the error, got: {message:?}"
        );
    }
}
