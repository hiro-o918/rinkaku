//! Composition root for the `rinkaku` binary.
//!
//! This is the only place allowed to know about the concrete CLI wiring.
//! It stays a thin entry point: parse arguments, obtain the diff text
//! (stdin or `git diff`), read changed files, and dispatch to the pure
//! core in `lib.rs` (`pipeline::analyze_diff`, `render::render`).
//!
//! The file-reading port passed to `analyze_diff` differs by input mode:
//!
//! - `--base` mode: the diff comes from `git diff <base>...<head>`, so
//!   files are read via `git show <head>:<path>` rather than off the
//!   working tree. This keeps the diff and the file content read from the
//!   exact same commit by construction, regardless of what the working
//!   tree currently holds (uncommitted changes, a dirty checkout, etc.).
//! - stdin mode: the diff's provenance is unknown to rinkaku (it could be
//!   `gh pr diff`, a saved patch file, anything). Files are read off the
//!   working tree, under the assumption that **the diff is consistent
//!   with the current working tree** — i.e. applying it (or having
//!   already applied it) would reproduce the working tree's content. If
//!   that assumption doesn't hold, line numbers in the extracted symbols
//!   may not line up with the actual file content.

use clap::Parser;
use rinkaku_core::deps::TagsResolver;
use rinkaku_core::language::language_for_path;
use rinkaku_core::pipeline::analyze_diff;
use rinkaku_core::render::{OutputFormat, render};
use std::io::IsTerminal;
use std::io::Read;

/// rinkaku (輪郭) — condense PR diffs into signatures and their dependencies.
#[derive(Parser, Debug, PartialEq, Eq)]
#[command(name = "rinkaku", version, about, long_about = None)]
struct Cli {
    /// Base ref to diff against (runs `git diff <base>...<head>` instead
    /// of reading from stdin).
    #[arg(long)]
    base: Option<String>,

    /// Head ref to diff against `base`. Only meaningful together with
    /// `--base`; defaults to `HEAD`.
    #[arg(long, default_value = "HEAD")]
    head: String,

    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Md)]
    format: Format,

    /// Whether to resolve each changed symbol's 1-hop dependencies
    /// (ADR 0003). `1` (default) runs the tags-based `Resolver` over
    /// every file tracked by `git ls-files`; `0` skips resolution
    /// entirely (no `Resolver::resolve` calls), which is faster and
    /// avoids the repo-wide indexing pass.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=1))]
    deps: u8,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Md,
    Json,
}

impl From<Format> for OutputFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Md => OutputFormat::Markdown,
            Format::Json => OutputFormat::Json,
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    let report = match &cli.base {
        Some(base) => {
            let diff_text = run_git_diff(base, &cli.head)?;
            let head = cli.head.clone();
            let read_file = {
                let head = head.clone();
                move |path: &str| read_git_show_file(None, &head, path)
            };
            let resolver = build_resolver(&cli, &diff_text, &read_file, Some(&head), None)?;
            analyze_diff(
                &diff_text,
                read_file,
                resolver
                    .as_ref()
                    .map(|r| r as &dyn rinkaku_core::deps::Resolver),
            )?
        }
        None => {
            let diff_text = read_stdin_diff()?;
            if diff_text.trim().is_empty() {
                eprintln!("note: diff is empty, nothing to analyze");
            }
            let resolver = build_resolver(&cli, &diff_text, read_working_tree_file, None, None)?;
            let report = analyze_diff(
                &diff_text,
                read_working_tree_file,
                resolver
                    .as_ref()
                    .map(|r| r as &dyn rinkaku_core::deps::Resolver),
            )?;
            // Garbage stdin input (not empty, but not a unified diff
            // either — e.g. a plain text file piped in by mistake) parses
            // to zero recognized file entries: `parse_unified_diff` never
            // errors on unrecognized text, it simply finds nothing to
            // report (see `diff.rs`), so this would otherwise exit 0 with
            // an empty report and no indication anything went wrong. Only
            // checked when the input wasn't already flagged as empty
            // above, and only for non-whitespace input, so this note and
            // the empty-diff note above are mutually exclusive.
            if !diff_text.trim().is_empty() && report.files.is_empty() && report.skipped.is_empty()
            {
                eprintln!("note: no file changes recognized in input; expected a unified diff");
            }
            report
        }
    };

    let output = render(&report, cli.format.into())?;
    print!("{output}");

    Ok(())
}

/// Builds the `TagsResolver` used for `--deps 1` (the default), or `None`
/// when `--deps 0` skips dependency resolution entirely.
///
/// Indexes every file `git ls-files` reports as tracked — untracked files
/// are excluded by construction (not merely `.gitignore`-filtered, since
/// `ls-files` only ever lists tracked paths in the first place) — so the
/// index only ever contains content the repository actually owns.
///
/// Before indexing, `diff_text` is parsed once (via
/// `pipeline::collect_referenced_names`, reading changed files through
/// `diff_read_file`) to compute the set of names any changed symbol
/// actually references. That set drives `TagsResolver::new`'s prefilter:
/// only tracked files whose content could plausibly contain one of those
/// names get parsed at all (see `deps.rs`'s performance doc comment for
/// why this cannot lose recall). This re-parses the diff and re-reads
/// changed files a second time (`analyze_diff` does its own pass over the
/// same diff right after `build_resolver` returns) — accepted the same
/// way `analyze_diff`'s doc comment already accepts `TagsResolver::new`
/// parsing changed files a second time for its index.
///
/// `head`, when `Some`, matches `--base` mode's read strategy: file
/// content is read via `git show <head>:<path>` rather than the working
/// tree, so the index and the diff being analyzed are consistent with the
/// same commit regardless of the working tree's state (same rationale as
/// `read_git_show_file`, applied here to the whole repo rather than just
/// the changed files).
/// `cwd` selects the repository `list_git_files` runs `git ls-files` in
/// (same rationale as `read_git_show_file`'s `cwd`: `None` uses the
/// process's current directory for production callers, `Some(dir)` pins
/// it for tests). Only reached when `cli.deps != 0` — the `deps == 0`
/// branch returns before doing any repository scan at all, verified by
/// `should_skip_git_ls_files_when_deps_is_zero` below (pointing `cwd` at
/// a directory with no git repository would make `list_git_files` fail,
/// so a passing `Ok(None)` there is proof the scan never ran).
fn build_resolver(
    cli: &Cli,
    diff_text: &str,
    diff_read_file: impl Fn(&str) -> std::io::Result<String>,
    head: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<Option<TagsResolver>> {
    if cli.deps == 0 {
        return Ok(None);
    }

    let reference_names =
        rinkaku_core::pipeline::collect_referenced_names(diff_text, diff_read_file)?;

    let paths = list_git_files(cwd)?;
    let files = paths.into_iter().filter_map(|path| {
        let content = match head {
            Some(head) => read_git_show_file(cwd, head, &path),
            None => read_working_tree_file(&path),
        };
        // A file listed by `git ls-files` can still fail to read (e.g.
        // deleted in the working tree but not yet staged, a submodule
        // gitlink entry) — skipped rather than failing the whole run,
        // since the resolver's index is a best-effort aid, not a
        // correctness-critical input.
        content.ok().map(|content| (path, content))
    });
    Ok(Some(TagsResolver::new(
        files,
        language_for_path,
        &reference_names,
    )))
}

/// Lists every file tracked by git in `cwd` (or the process's current
/// directory when `None`) via `git ls-files`.
fn list_git_files(cwd: Option<&std::path::Path>) -> anyhow::Result<Vec<String>> {
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

/// Reads the diff from stdin. Errors with a clear message if stdin is a
/// terminal (interactive), since there is nothing to read in that case and
/// `--base` should be used instead.
fn read_stdin_diff() -> anyhow::Result<String> {
    if std::io::stdin().is_terminal() {
        anyhow::bail!(
            "no diff input: pipe a diff via stdin (e.g. `gh pr diff 123 | rinkaku`) or pass --base <ref>"
        );
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

/// Runs `git diff <base>...<head>` and returns its stdout.
fn run_git_diff(base: &str, head: &str) -> anyhow::Result<String> {
    let range = format!("{base}...{head}");
    let output = std::process::Command::new("git")
        .args(["diff", &range])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git diff {range} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Reads a changed file's new-side content off the working tree.
fn read_working_tree_file(path: &str) -> std::io::Result<String> {
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
fn read_git_show_file(
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
    use pretty_assertions::assert_eq;

    #[test]
    fn should_default_to_markdown_head_and_no_base_when_no_args_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_base_when_base_flag_given() {
        let expected = Cli {
            base: Some("main".to_string()),
            head: "HEAD".to_string(),
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_base_and_head_when_both_flags_given() {
        let expected = Cli {
            base: Some("main".to_string()),
            head: "feature-branch".to_string(),
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main", "--head", "feature-branch"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_format_json_when_format_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            format: Format::Json,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "--format", "json"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_unknown_format_value() {
        let actual = Cli::try_parse_from(["rinkaku", "--format", "yaml"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_deps_zero_when_deps_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            format: Format::Md,
            deps: 0,
        };
        let actual = Cli::parse_from(["rinkaku", "--deps", "0"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_deps_value_outside_zero_or_one() {
        let actual = Cli::try_parse_from(["rinkaku", "--deps", "2"]);

        assert!(actual.is_err());
    }

    /// Runs `git` inside `dir`, panicking with the captured stderr on
    /// failure. Test-only helper: production code never wants a panicking
    /// git wrapper.
    fn run_git(dir: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must be installed to run this test");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Sets up a throwaway git repository with deterministic author/committer
    /// identity (avoids depending on the host's global git config) and one
    /// commit containing `src/lib.rs` with `content`.
    fn init_repo_with_committed_file(dir: &std::path::Path, content: &str) {
        run_git(dir, &["init", "--initial-branch=main"]);
        run_git(dir, &["config", "user.email", "test@example.com"]);
        run_git(dir, &["config", "user.name", "Test"]);
        std::fs::create_dir_all(dir.join("src")).expect("create src dir");
        std::fs::write(dir.join("src/lib.rs"), content).expect("write src/lib.rs");
        run_git(dir, &["add", "src/lib.rs"]);
        run_git(dir, &["commit", "-m", "initial commit"]);
    }

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

    // Regression test for the must-fix performance bug: `build_resolver`
    // must return before doing any repository scan when `deps == 0`. This
    // is exercised indirectly rather than by inspecting call counts (no
    // mocking of `git`, per this project's test conventions): `cwd` points
    // at a plain (non-git) tempdir, so if `list_git_files` were reached,
    // `git ls-files` would fail there and `build_resolver` would return
    // `Err`. Observing `Ok(None)` is therefore proof the scan never ran.
    //
    // NOTE: partial assertion (`is_none()` rather than a fully qualified
    // comparison) because `TagsResolver` derives neither `Debug` nor
    // `PartialEq` — its `HashMap` index isn't meant to be compared as a
    // value, only used through `Resolver::resolve`. Which variant of
    // `Option` came back is exactly what this test needs to know.
    #[test]
    fn should_skip_repository_scan_when_deps_is_zero() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let cli = Cli {
            base: None,
            head: "HEAD".to_string(),
            format: Format::Md,
            deps: 0,
        };
        // Never called if `deps == 0` truly short-circuits before doing
        // any work at all — deliberately panics so a regression that
        // starts calling it would fail loudly rather than silently
        // reading an empty string.
        let read_file = |_: &str| -> std::io::Result<String> {
            panic!("read_file must not be called when deps == 0")
        };

        let actual = build_resolver(&cli, "", read_file, None, Some(dir.path()))
            .expect("deps == 0 must not touch the repository at all");

        assert!(actual.is_none());
    }

    // Sibling case to the one above: with `deps == 1` (repository scan
    // enabled), the same non-git `cwd` makes `list_git_files` fail,
    // confirming the scan is actually attempted in this branch and that
    // the `Ok(None)` above is specific to `deps == 0`, not an artifact of
    // the test directory itself.
    #[test]
    fn should_fail_when_deps_is_one_and_cwd_has_no_git_repository() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let cli = Cli {
            base: None,
            head: "HEAD".to_string(),
            format: Format::Md,
            deps: 1,
        };
        let read_file = |_: &str| -> std::io::Result<String> { Ok(String::new()) };

        let actual = build_resolver(&cli, "", read_file, None, Some(dir.path()));

        assert!(actual.is_err());
    }
}
