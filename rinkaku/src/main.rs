//! Composition root for the `rinkaku` binary.
//!
//! This is the only place allowed to know about the concrete CLI wiring.
//! It stays a thin entry point: parse arguments, obtain the diff text
//! (stdin, `git diff`, or a resolved PR), read changed files, and
//! dispatch to the pure core in `lib.rs` (`pipeline::analyze_diff`,
//! `render::render`).
//!
//! The file-reading port passed to `analyze_diff` differs by input mode:
//!
//! - `--base` mode: the diff comes from `git diff <base>...<head>`, so
//!   files are read via `git show <head>:<path>` rather than off the
//!   working tree. This keeps the diff and the file content read from the
//!   exact same commit by construction, regardless of what the working
//!   tree currently holds (uncommitted changes, a dirty checkout, etc.).
//! - `--pr` mode (ADR 0004): the PR's base branch and head commit are
//!   resolved via `gh pr view`, both are fetched into the local clone
//!   with `git fetch`, and the resulting base/head SHAs are handed to
//!   exactly the same `git show`-backed read strategy as `--base` mode —
//!   `--pr` is a resolution step in front of the `--base` pipeline, not a
//!   separate read strategy. Requires running inside a local clone whose
//!   `origin` remote is the target repository, and requires `gh` to be
//!   installed and authenticated.
//! - stdin mode: the diff's provenance is unknown to rinkaku (it could be
//!   `gh pr diff`, a saved patch file, anything). Files are read off the
//!   working tree, under the assumption that **the diff is consistent
//!   with the current working tree** — i.e. applying it (or having
//!   already applied it) would reproduce the working tree's content. If
//!   that assumption doesn't hold, line numbers in the extracted symbols
//!   may not line up with the actual file content.

mod self_update;

use clap::{Parser, Subcommand};
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
    /// Subcommand to run. Omitted for the default diff-condensation flow
    /// (stdin / `--base` / `--deps` / `--format` below), which stays the
    /// primary, backward-compatible entry point.
    #[command(subcommand)]
    command: Option<Command>,

    /// Base ref to diff against (runs `git diff <base>...<head>` instead
    /// of reading from stdin).
    #[arg(long, conflicts_with = "pr")]
    base: Option<String>,

    /// Head ref to diff against `base`. Only meaningful together with
    /// `--base`; defaults to `HEAD`.
    //
    // `conflicts_with = "pr"` only fires when `--head` is explicitly
    // passed (clap does not treat a default value as "provided"), which
    // is exactly what's wanted: `--pr` resolves its own head commit via
    // `gh`, so an explicit `--head` alongside `--pr` would be silently
    // ignored otherwise.
    #[arg(long, default_value = "HEAD", conflicts_with = "pr")]
    head: String,

    /// GitHub PR to review, as a URL
    /// (`https://github.com/<owner>/<repo>/pull/<number>`) or a bare PR
    /// number (`76`). Must be run inside a local clone of the target
    /// repository, with `gh` installed and authenticated.
    // See ADR 0004 for the resolve-then-fetch design this drives in `main`.
    #[arg(long)]
    pr: Option<String>,

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

#[derive(Subcommand, Debug, PartialEq, Eq)]
enum Command {
    /// Update rinkaku to the latest GitHub release in place. If you
    /// installed via Homebrew or `cargo install`, prefer `brew upgrade`
    /// or `cargo install rinkaku` instead so your package manager stays
    /// in sync — self-update works either way, but it bypasses those
    /// managers' bookkeeping.
    ///
    /// Requires either an interactive terminal (to confirm the update) or
    /// `--yes`. Refuses to run when stdin is not a TTY and `--yes` is not
    /// given, since there would be no one to answer the confirmation
    /// prompt.
    SelfUpdate {
        /// Skip the interactive confirmation prompt and proceed.
        #[arg(long, short = 'y')]
        yes: bool,
    },
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

    if let Some(Command::SelfUpdate { yes }) = cli.command {
        return self_update::run_self_update(yes);
    }

    let report = if let Some(pr_arg) = &cli.pr {
        // Validate the arg and derive the fetch refspec's PR number, but
        // pass the original (trimmed) value — not the parsed number — to
        // `gh pr view` (see that function's doc comment for why).
        let number = parse_pr_arg(pr_arg)?;
        let pr_info = fetch_pr_info(pr_arg.trim())?;
        let head_sha = fetch_pr_head(number)?;
        if head_sha != pr_info.head_ref_oid {
            anyhow::bail!(
                "fetched PR #{number} head ({head_sha}) does not match `gh`'s reported head \
                 ({expected}); this usually means the PR belongs to a different repository than \
                 this clone's `origin` remote, or the PR was updated between resolving it and \
                 fetching it — verify `origin` points at the PR's repository and re-run",
                expected = pr_info.head_ref_oid,
            );
        }
        let base_sha = fetch_branch_head(&pr_info.base_ref_name)?;
        run_base_pipeline(&cli, &base_sha, &head_sha)?
    } else if let Some(base) = &cli.base {
        run_base_pipeline(&cli, base, &cli.head)?
    } else {
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
        if let Some(note) = garbage_input_note(&diff_text, &report) {
            eprintln!("{note}");
        }
        report
    };

    let output = render(&report, cli.format.into())?;
    print!("{output}");

    Ok(())
}

/// Runs `git diff <base>...<head>` and analyzes the result, reading file
/// content via `git show <head>:<path>` (ADR: keeps the diff and file
/// reads pinned to the same commit regardless of the working tree's
/// state). Shared by `--base` mode (`base`/`head` are the ref strings the
/// user passed) and `--pr` mode (`base`/`head` are the SHAs resolved and
/// fetched from the PR, see ADR 0004) — `--pr` is a resolution step in
/// front of this same pipeline, not a separate read strategy.
fn run_base_pipeline(
    cli: &Cli,
    base: &str,
    head: &str,
) -> anyhow::Result<rinkaku_core::render::Report> {
    let diff_text = run_git_diff(base, head)?;
    let read_file = {
        let head = head.to_string();
        move |path: &str| read_git_show_file(None, &head, path)
    };
    let resolver = build_resolver(cli, &diff_text, &read_file, Some(head), None)?;
    Ok(analyze_diff(
        &diff_text,
        read_file,
        resolver
            .as_ref()
            .map(|r| r as &dyn rinkaku_core::deps::Resolver),
    )?)
}

/// Returns a warning note for stdin input that is garbage rather than a
/// unified diff — non-empty input that nonetheless produced zero
/// recognized file entries (`parse_unified_diff` never errors on
/// unrecognized text, it simply finds nothing to report, see `diff.rs`),
/// which would otherwise silently exit 0 with an empty report and no
/// indication anything went wrong. `None` when `diff_text` is empty or
/// whitespace-only (already covered by the separate "diff is empty" note
/// at the call site — the two notes are mutually exclusive) or when the
/// report has any file or skip entry at all.
fn garbage_input_note(
    diff_text: &str,
    report: &rinkaku_core::render::Report,
) -> Option<&'static str> {
    if diff_text.trim().is_empty() {
        return None;
    }
    if !report.files.is_empty() || !report.skipped.is_empty() {
        return None;
    }
    Some("note: no file changes recognized in input; expected a unified diff")
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

/// The subset of `gh pr view --json number,baseRefName,headRefOid` this
/// binary needs to drive `--pr` mode (ADR 0004): which PR, what its base
/// branch is called, and the exact commit its head is expected to be at
/// (checked against what `git fetch` actually retrieves, see
/// `main`'s mismatch check).
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
struct PrInfo {
    number: u64,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
}

/// Extracts a PR number from `--pr`'s value: either a bare number
/// (`"76"`) or a GitHub PR URL
/// (`https://github.com/<owner>/<repo>/pull/<number>`, tolerating a
/// trailing slash or extra path segments like `/files`).
///
/// `0` is rejected even though it parses as a `u64`: GitHub PR numbers
/// are 1-indexed, so `0` can only be a typo, and failing fast here beats
/// a confusing `gh pr view 0` error downstream.
fn parse_pr_arg(value: &str) -> anyhow::Result<u64> {
    let candidate = match value.trim().strip_prefix("https://github.com/") {
        Some(rest) => {
            // Expect `<owner>/<repo>/pull/<number>[/...]`.
            let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
            match segments.as_slice() {
                [_owner, _repo, "pull", number, ..] => *number,
                _ => anyhow::bail!(
                    "--pr URL must look like https://github.com/<owner>/<repo>/pull/<number>, \
                     got: {value}"
                ),
            }
        }
        None => value.trim(),
    };

    let number: u64 = candidate.parse().map_err(|_| {
        anyhow::anyhow!("--pr must be a PR number or a GitHub PR URL, got: {value}")
    })?;
    if number == 0 {
        anyhow::bail!("--pr must be a positive PR number, got: {value}");
    }
    Ok(number)
}

/// Parses `gh pr view --json number,baseRefName,headRefOid`'s stdout.
/// Split out from `fetch_pr_info` so the JSON shape can be unit-tested
/// without shelling out to `gh`.
fn parse_pr_view_json(json: &str) -> anyhow::Result<PrInfo> {
    Ok(serde_json::from_str(json)?)
}

/// Runs `gh pr view <arg> --json number,baseRefName,headRefOid` and parses
/// the result.
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
fn fetch_pr_info(arg: &str) -> anyhow::Result<PrInfo> {
    let output = std::process::Command::new("gh")
        .args(["pr", "view", arg, "--json", "number,baseRefName,headRefOid"])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "gh pr view {arg} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    parse_pr_view_json(&String::from_utf8(output.stdout)?)
}

/// Fetches PR `number`'s head ref into the local clone and returns the
/// fetched commit's SHA, via `git fetch origin refs/pull/<number>/head`
/// followed by `git rev-parse FETCH_HEAD`.
fn fetch_pr_head(number: u64) -> anyhow::Result<String> {
    run_git_fetch(&format!("refs/pull/{number}/head"))
}

/// Fetches branch `name` into the local clone and returns the fetched
/// commit's SHA. Used to resolve `--pr` mode's base commit from the base
/// branch name `gh pr view` reports.
fn fetch_branch_head(name: &str) -> anyhow::Result<String> {
    run_git_fetch(name)
}

/// Runs `git fetch origin <refspec>` then `git rev-parse FETCH_HEAD`,
/// returning the resulting SHA. Shared by `fetch_pr_head` and
/// `fetch_branch_head`, which differ only in what refspec they fetch.
fn run_git_fetch(refspec: &str) -> anyhow::Result<String> {
    let fetch_output = std::process::Command::new("git")
        .args(["fetch", "origin", refspec])
        .output()?;
    if !fetch_output.status.success() {
        anyhow::bail!(
            "git fetch origin {refspec} failed: {}",
            String::from_utf8_lossy(&fetch_output.stderr)
        );
    }

    let rev_parse_output = std::process::Command::new("git")
        .args(["rev-parse", "FETCH_HEAD"])
        .output()?;
    if !rev_parse_output.status.success() {
        anyhow::bail!(
            "git rev-parse FETCH_HEAD failed after fetching {refspec}: {}",
            String::from_utf8_lossy(&rev_parse_output.stderr)
        );
    }
    Ok(String::from_utf8(rev_parse_output.stdout)?
        .trim()
        .to_string())
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
    use rstest::rstest;

    #[test]
    fn should_default_to_markdown_head_and_no_base_when_no_args_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_base_when_base_flag_given() {
        let expected = Cli {
            command: None,
            base: Some("main".to_string()),
            head: "HEAD".to_string(),
            pr: None,
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_base_and_head_when_both_flags_given() {
        let expected = Cli {
            command: None,
            base: Some("main".to_string()),
            head: "feature-branch".to_string(),
            pr: None,
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main", "--head", "feature-branch"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_format_json_when_format_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
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
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
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

    #[test]
    fn should_set_self_update_command_when_self_update_subcommand_given() {
        let expected = Cli {
            command: Some(Command::SelfUpdate { yes: false }),
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "self-update"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_yes_flag_when_self_update_yes_flag_given() {
        let expected = Cli {
            command: Some(Command::SelfUpdate { yes: true }),
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "self-update", "--yes"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_yes_flag_when_self_update_short_y_flag_given() {
        let expected = Cli {
            command: Some(Command::SelfUpdate { yes: true }),
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "self-update", "-y"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_verify_cli_definition() {
        // clap's own consistency check (duplicate args, invalid
        // configuration, etc.) — mirrors skem's `Cli::command().debug_assert()`
        // convention for catching CLI wiring mistakes at test time.
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn should_set_pr_when_pr_flag_given() {
        // Also covers that `--pr` alone (no explicit `--head`) parses
        // successfully: `--head` has a default value, so clap's
        // `conflicts_with` must not fire unless `--head` was actually
        // passed on the command line — this is the behavior the ADR relies
        // on to let `--pr` reuse the `Cli` struct's `head` field internally
        // without users needing to omit an unrelated flag.
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: Some("76".to_string()),
            format: Format::Md,
            deps: 1,
        };
        let actual = Cli::parse_from(["rinkaku", "--pr", "76"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_pr_and_base_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--pr", "76", "--base", "main"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_reject_pr_and_explicit_head_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--pr", "76", "--head", "feature-branch"]);

        assert!(actual.is_err());
    }

    #[rstest]
    #[case::should_parse_bare_number("76", 76)]
    #[case::should_parse_number_with_surrounding_whitespace(" 76 ", 76)]
    #[case::should_parse_pull_url("https://github.com/octocat/hello-world/pull/123", 123)]
    #[case::should_parse_pull_url_with_trailing_slash(
        "https://github.com/octocat/hello-world/pull/123/",
        123
    )]
    #[case::should_parse_pull_url_with_extra_path_segment(
        "https://github.com/octocat/hello-world/pull/123/files",
        123
    )]
    fn should_parse_pr_arg_when_input_is_valid(#[case] input: &str, #[case] expected: u64) {
        let actual = parse_pr_arg(input).expect("expected a valid PR number");

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_reject_empty_string("")]
    #[case::should_reject_non_numeric_string("abc")]
    #[case::should_reject_zero("0")]
    #[case::should_reject_negative_number("-1")]
    #[case::should_reject_non_pull_github_url("https://github.com/octocat/hello-world/issues/123")]
    #[case::should_reject_github_url_missing_number("https://github.com/octocat/hello-world/pull/")]
    #[case::should_reject_unrelated_url("https://example.com/pull/123")]
    fn should_reject_pr_arg_when_input_is_invalid(#[case] input: &str) {
        let actual = parse_pr_arg(input);

        assert!(actual.is_err(), "expected an error for input: {input}");
    }

    #[test]
    fn should_parse_pr_view_json_into_pr_info() {
        let json = r#"{"number":123,"baseRefName":"main","headRefOid":"abc123def456"}"#;

        let actual = parse_pr_view_json(json).expect("expected valid JSON to parse");

        assert_eq!(
            PrInfo {
                number: 123,
                base_ref_name: "main".to_string(),
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
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
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
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: Format::Md,
            deps: 1,
        };
        let read_file = |_: &str| -> std::io::Result<String> { Ok(String::new()) };

        let actual = build_resolver(&cli, "", read_file, None, Some(dir.path()));

        assert!(actual.is_err());
    }

    mod garbage_input_note_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_core::render::Report;

        fn empty_report() -> Report {
            Report {
                files: vec![],
                skipped: vec![],
            }
        }

        fn non_empty_report() -> Report {
            Report {
                files: vec![rinkaku_core::render::FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![],
                }],
                skipped: vec![],
            }
        }

        #[test]
        fn should_return_note_when_input_is_non_empty_but_report_has_no_entries() {
            let actual = garbage_input_note("this is not a diff at all\n", &empty_report());

            assert_eq!(
                Some("note: no file changes recognized in input; expected a unified diff"),
                actual
            );
        }

        #[test]
        fn should_return_none_when_input_is_empty() {
            let actual = garbage_input_note("", &empty_report());

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_input_is_whitespace_only() {
            let actual = garbage_input_note("   \n\n  ", &empty_report());

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_report_has_file_entries() {
            let actual = garbage_input_note("some diff text", &non_empty_report());

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_report_has_only_skipped_entries() {
            let report = Report {
                files: vec![],
                skipped: vec![rinkaku_core::render::SkippedFile {
                    path: "assets/logo.png".to_string(),
                    reason: rinkaku_core::render::SkipReason::Binary,
                }],
            };

            let actual = garbage_input_note("some diff text", &report);

            assert_eq!(None, actual);
        }
    }
}
