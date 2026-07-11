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
            analyze_diff(&diff_text, move |path| {
                read_git_show_file(None, &head, path)
            })?
        }
        None => {
            let diff_text = read_stdin_diff()?;
            if diff_text.trim().is_empty() {
                eprintln!("note: diff is empty, nothing to analyze");
            }
            analyze_diff(&diff_text, read_working_tree_file)?
        }
    };

    let output = render(&report, cli.format.into())?;
    print!("{output}");

    Ok(())
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
        };
        let actual = Cli::parse_from(["rinkaku", "--format", "json"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_unknown_format_value() {
        let actual = Cli::try_parse_from(["rinkaku", "--format", "yaml"]);

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
}
