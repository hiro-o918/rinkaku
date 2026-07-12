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
//! - `--pr` mode (ADR 0004, ADR 0005, ADR 0006, ADR 0007): the PR's base
//!   branch, base commit (`baseRefOid`), and head commit are resolved via
//!   `gh pr view`; the head is fetched with `git fetch`, and the base
//!   commit is resolved via `baseRefOid` rather than the base branch's
//!   current tip (ADR 0007) — this is what makes `--pr` work on a merged
//!   PR, whose base branch has since advanced past the PR's own commits.
//!   The resulting base/head SHAs are handed to exactly the same
//!   `git show`-backed read strategy as `--base` mode — `--pr` is a
//!   resolution step in front of the `--base` pipeline, not a separate
//!   read strategy. A bare PR number requires running inside a local
//!   clone of the target repository. A PR URL also uses the current
//!   directory when its `origin` matches the URL's repository; otherwise
//!   it prefers an existing `ghq`-managed clone of the repository when one
//!   is found (ADR 0006), and only falls back to auto-cloning a blobless
//!   partial clone into a per-repository cache directory (ADR 0005) if
//!   neither the cwd nor `ghq` has one — so URL input works from any
//!   directory either way. `gh` must be installed and authenticated
//!   either way.
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
    /// number (`76`). A bare number must be run inside a local clone of
    /// the target repository; a URL also works from any other directory
    /// by auto-cloning into a cache. Requires `gh` installed and
    /// authenticated.
    // See ADR 0004 for the resolve-then-fetch design and ADR 0005 for the
    // auto-clone-into-cache behavior this drives in `main`.
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
        let parsed = parse_pr_arg(pr_arg)?;
        let number = parsed.number();
        let workdir = resolve_pr_workdir(&parsed)?;
        let pr_info = fetch_pr_info(pr_arg.trim())?;
        let head_sha = fetch_pr_head(number, workdir.as_deref())?;
        if head_sha != pr_info.head_ref_oid {
            anyhow::bail!(
                "fetched PR #{number} head ({head_sha}) does not match `gh`'s reported head \
                 ({expected}); this usually means the PR belongs to a different repository than \
                 the target clone's `origin` remote, or the PR was updated between resolving it \
                 and fetching it — verify `origin` points at the PR's repository and re-run",
                expected = pr_info.head_ref_oid,
            );
        }
        let cwd = workdir.as_deref();
        let (base_sha, used_fallback) = resolve_pr_base_sha(
            &pr_info.base_ref_oid,
            |oid| object_exists_locally(cwd, oid),
            || fetch_branch_head(&pr_info.base_ref_name, cwd).map(|_| ()),
            |oid| fetch_oid(cwd, oid),
            || fetch_branch_head(&pr_info.base_ref_name, cwd),
        )?;
        if used_fallback {
            log::warn!(
                "could not resolve PR #{number}'s base commit ({base_oid}) locally; falling \
                 back to the current tip of {base_branch}, which may not reproduce the original \
                 PR diff for a merged PR",
                base_oid = pr_info.base_ref_oid,
                base_branch = pr_info.base_ref_name,
            );
        }
        run_base_pipeline(&cli, &base_sha, &head_sha, workdir.as_deref())?
    } else if let Some(base) = &cli.base {
        run_base_pipeline(&cli, base, &cli.head, None)?
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

/// Determines which repository `--pr` mode should run its `git` commands
/// in, and clones one into the cache if needed. Probes in order (ADR
/// 0006): (1) the current directory, (2) a ghq-managed clone, (3) the
/// cache clone (ADR 0005).
///
/// `PrArg::Number` always uses the process's current directory (`None`,
/// meaning "no override" to the `cwd` parameters downstream) — a bare
/// number carries no repository information, so ADR 0004's "run inside a
/// local clone" requirement is unchanged for it.
///
/// `PrArg::Url` first checks whether the current directory is already a
/// clone of that repository (`git remote get-url origin` matching
/// `owner`/`repo`, case-insensitively via `github_remote_matches`); if so
/// it also returns `None`, reusing the cwd exactly like `PrArg::Number`
/// does today. Otherwise it asks `ghq list --full-path --exact
/// <owner>/<repo>` for candidate clones (`ghq_candidate_clones`) and
/// picks the first whose real origin matches (`select_matching_clone`,
/// resolving each candidate's origin via `git_remote_origin_url`); ghq
/// being absent, erroring, or returning only non-matching clones all
/// fall through silently to this step returning `None` (per ADR 0006 —
/// `ghq_candidate_clones` already logs the reason at debug level). Only
/// if neither the cwd nor a ghq clone matches does it fall back to the
/// per-repository cache directory (`cache_repo_dir`, reading the real
/// environment here at the boundary) and clone into it if it doesn't
/// exist yet — an existing cache entry, like a discovered ghq clone, is
/// left alone here and refreshed by the `git fetch` calls `main` makes
/// afterwards, not re-cloned.
fn resolve_pr_workdir(parsed: &PrArg) -> anyhow::Result<Option<std::path::PathBuf>> {
    let PrArg::Url { owner, repo, .. } = parsed else {
        return Ok(None);
    };

    if let Some(origin) = git_remote_origin_url(None)?
        && github_remote_matches(&origin, owner, repo)
    {
        return Ok(None);
    }

    let ghq_candidates = ghq_candidate_clones(owner, repo);
    if let Some(discovered) = select_matching_clone(
        &ghq_candidates,
        |path| git_remote_origin_url(Some(path)).ok().flatten(),
        owner,
        repo,
    ) {
        return Ok(Some(discovered));
    }

    let dir = cache_repo_dir(
        std::env::var("RINKAKU_CACHE_DIR").ok().as_deref(),
        std::env::var("XDG_CACHE_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        owner,
        repo,
    )?;
    if !dir.exists() {
        // Create the parent (`.../repos/github.com/<owner>/`) only, not
        // `dir` itself: `gh repo clone` (like `git clone`) creates its
        // destination directory and fails if it already exists.
        std::fs::create_dir_all(dir.parent().unwrap_or(&dir)).map_err(|source| {
            anyhow::anyhow!(
                "failed to create cache directory for {}: {source}",
                dir.display()
            )
        })?;
        clone_repo_into_cache(owner, repo, &dir)?;
    }
    Ok(Some(dir))
}

/// Runs `git diff <base>...<head>` and analyzes the result, reading file
/// content via `git show <head>:<path>` (ADR: keeps the diff and file
/// reads pinned to the same commit regardless of the working tree's
/// state). Shared by `--base` mode (`base`/`head` are the ref strings the
/// user passed) and `--pr` mode (`base`/`head` are the SHAs resolved and
/// fetched from the PR, see ADR 0004) — `--pr` is a resolution step in
/// front of this same pipeline, not a separate read strategy.
///
/// `cwd` selects which repository every subprocess in this call runs in
/// (same rationale as `read_git_show_file`'s `cwd`: `None` uses the
/// process's current directory for `--base` and cwd-clone `--pr` runs,
/// `Some(dir)` targets a cache clone (ADR 0005) or a test fixture).
fn run_base_pipeline(
    cli: &Cli,
    base: &str,
    head: &str,
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<rinkaku_core::render::Report> {
    let diff_text = run_git_diff(base, head, cwd)?;
    let read_file = {
        let head = head.to_string();
        move |path: &str| read_git_show_file(cwd, &head, path)
    };
    let resolver = build_resolver(cli, &diff_text, &read_file, Some(head), cwd)?;
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
///
/// `cwd` selects the repository to run `git` in; `None` uses the process's
/// current directory (production `--base`/cwd-clone `--pr` callers),
/// `Some(dir)` pins it (cache clones, tests).
fn run_git_diff(base: &str, head: &str, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
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

/// The subset of `gh pr view --json number,baseRefName,baseRefOid,
/// headRefOid` this binary needs to drive `--pr` mode (ADR 0004, ADR
/// 0007): which PR, what its base branch is called (fallback path),
/// the commit its base was pinned to at PR time (`base_ref_oid`,
/// preferred — see ADR 0007), and the exact commit its head is expected
/// to be at (checked against what `git fetch` actually retrieves, see
/// `main`'s mismatch check).
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
struct PrInfo {
    number: u64,
    #[serde(rename = "baseRefName")]
    base_ref_name: String,
    #[serde(rename = "baseRefOid")]
    base_ref_oid: String,
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
}

/// A validated `--pr` argument. `Url` carries `owner`/`repo` (not just the
/// PR number) so callers can decide, per ADR 0005, whether the current
/// directory's clone matches the PR's repository or a cache clone is
/// needed — information a bare `Number` inherently cannot provide, which
/// is exactly why `Number` still requires running inside a local clone.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PrArg {
    Number(u64),
    Url {
        owner: String,
        repo: String,
        number: u64,
    },
}

impl PrArg {
    /// The PR number, regardless of which variant this is. Used to build
    /// the `refs/pull/<number>/head` fetch refspec, which only needs the
    /// number even for `Url`.
    fn number(&self) -> u64 {
        match self {
            PrArg::Number(number) => *number,
            PrArg::Url { number, .. } => *number,
        }
    }
}

/// Extracts a validated `--pr` argument: either a bare number (`"76"`) or
/// a GitHub PR URL
/// (`https://github.com/<owner>/<repo>/pull/<number>`, tolerating a
/// trailing slash or extra path segments like `/files`).
///
/// `0` is rejected even though it parses as a `u64`: GitHub PR numbers
/// are 1-indexed, so `0` can only be a typo, and failing fast here beats
/// a confusing `gh pr view 0` error downstream.
fn parse_pr_arg(value: &str) -> anyhow::Result<PrArg> {
    match value.trim().strip_prefix("https://github.com/") {
        Some(rest) => {
            // Expect `<owner>/<repo>/pull/<number>[/...]`.
            let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
            match segments.as_slice() {
                [owner, repo, "pull", number, ..] => Ok(PrArg::Url {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    number: parse_positive_pr_number(number, value)?,
                }),
                _ => anyhow::bail!(
                    "--pr URL must look like https://github.com/<owner>/<repo>/pull/<number>, \
                     got: {value}"
                ),
            }
        }
        None => Ok(PrArg::Number(parse_positive_pr_number(
            value.trim(),
            value,
        )?)),
    }
}

/// Parses `candidate` as a positive `u64` PR number, reporting errors
/// against the original (untrimmed/un-extracted) `--pr` value so the user
/// sees what they actually typed.
fn parse_positive_pr_number(candidate: &str, original_value: &str) -> anyhow::Result<u64> {
    let number: u64 = candidate.parse().map_err(|_| {
        anyhow::anyhow!("--pr must be a PR number or a GitHub PR URL, got: {original_value}")
    })?;
    if number == 0 {
        anyhow::bail!("--pr must be a positive PR number, got: {original_value}");
    }
    Ok(number)
}

/// Extracts `(owner, repo)` from a git remote URL, if it points at
/// GitHub. Accepts the forms `git remote get-url` can return for a GitHub
/// remote: `https://github.com/<owner>/<repo>`, the same with a `.git`
/// suffix, the scp-like SSH form `git@github.com:<owner>/<repo>(.git)`,
/// and the explicit `ssh://` form `ssh://git@github.com/<owner>/<repo>
/// (.git)`. Any other host, or a string that doesn't parse as one of
/// these forms, yields `None` — used by `main` to decide whether the
/// current directory's `origin` matches a `--pr` URL's repository (ADR
/// 0005), where "not GitHub" and "malformed" are both simply "no match".
fn parse_github_remote(url: &str) -> Option<(String, String)> {
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
fn github_remote_matches(remote_url: &str, owner: &str, repo: &str) -> bool {
    match parse_github_remote(remote_url) {
        Some((remote_owner, remote_repo)) => {
            remote_owner.eq_ignore_ascii_case(owner) && remote_repo.eq_ignore_ascii_case(repo)
        }
        None => false,
    }
}

/// Resolves the root cache directory for `--pr` URL auto-clones (ADR
/// 0005), then the per-repository clone path under it.
///
/// Precedence for the root, evaluated in order: `rinkaku_cache_dir`
/// (`$RINKAKU_CACHE_DIR`) if set, else `<xdg_cache_home>/rinkaku`
/// (`$XDG_CACHE_HOME/rinkaku`) if set, else `<home>/.cache/rinkaku`. An
/// error if none of the three inputs is available — there is then no
/// sane place to put the cache. All three are taken as arguments rather
/// than read from the environment here, so this stays a pure function the
/// precedence order can be unit-tested against directly; `main` is the
/// only place that reads the actual environment.
///
/// Repo layout under the root: `repos/github.com/<owner>/<repo>`, so
/// different git hosts (a future extension) could share one cache root
/// without path collisions, and so the cache directory's own contents
/// read as self-explanatory if a user goes looking (`~/.cache/rinkaku/
/// repos/github.com/octocat/hello-world`).
fn cache_repo_dir(
    rinkaku_cache_dir: Option<&str>,
    xdg_cache_home: Option<&str>,
    home: Option<&str>,
    owner: &str,
    repo: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let root = if let Some(dir) = rinkaku_cache_dir {
        std::path::PathBuf::from(dir)
    } else if let Some(dir) = xdg_cache_home {
        std::path::PathBuf::from(dir).join("rinkaku")
    } else if let Some(dir) = home {
        std::path::PathBuf::from(dir).join(".cache").join("rinkaku")
    } else {
        anyhow::bail!(
            "cannot determine a cache directory for --pr: set $RINKAKU_CACHE_DIR, \
             $XDG_CACHE_HOME, or $HOME"
        );
    };
    Ok(root.join("repos").join("github.com").join(owner).join(repo))
}

/// Parses `gh pr view --json number,baseRefName,baseRefOid,headRefOid`'s
/// stdout. Split out from `fetch_pr_info` so the JSON shape can be
/// unit-tested without shelling out to `gh`.
fn parse_pr_view_json(json: &str) -> anyhow::Result<PrInfo> {
    Ok(serde_json::from_str(json)?)
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
fn fetch_pr_info(arg: &str) -> anyhow::Result<PrInfo> {
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

/// Fetches PR `number`'s head ref into the repository at `cwd` and
/// returns the fetched commit's SHA, via
/// `git fetch origin refs/pull/<number>/head` followed by
/// `git rev-parse FETCH_HEAD`.
fn fetch_pr_head(number: u64, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
    run_git_fetch(&format!("refs/pull/{number}/head"), cwd)
}

/// Fetches branch `name` into the repository at `cwd` and returns the
/// fetched commit's SHA. Used to resolve `--pr` mode's base commit from
/// the base branch name `gh pr view` reports.
fn fetch_branch_head(name: &str, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
    run_git_fetch(name, cwd)
}

/// Resolves `--pr` mode's diff base commit following ADR 0007's
/// availability cascade, preferring `base_ref_oid` (pinned to the PR-time
/// base, correct for both open and merged PRs) over the base branch's
/// current tip (correct only for open PRs, since a merged PR's base
/// branch has since advanced past it).
///
/// Cascade, each step only taken if the previous one didn't already
/// resolve `base_ref_oid` locally:
///
/// 1. `object_exists` (`git cat-file -e <oid>^{commit}`) — already have it.
/// 2. `fetch_base_branch` (`git fetch origin <base_ref_name>`) then
///    re-check `object_exists` — an ordinary branch fetch usually retrieves
///    it, since `base_ref_oid` is normally reachable from the base
///    branch's history.
/// 3. `fetch_oid` (`git fetch origin <oid>`) then re-check `object_exists`
///    — covers a base branch that has since been force-pushed past it or
///    deleted.
/// 4. Fall back to `branch_tip` (today's pre-ADR-0007 behavior, e.g. an
///    already-fetched `fetch_branch_head` result) with `used_fallback`
///    signaling the caller should warn — the commit is unreachable by any
///    means available, so this degrades rather than fails the whole run.
///
/// Every IO step is injected as a closure so this decision logic is
/// unit-testable without shelling out to `git`, following the same
/// pattern as `select_matching_clone` elsewhere in this file.
///
/// Returns the resolved SHA and whether the fallback (step 4) was used.
fn resolve_pr_base_sha(
    base_ref_oid: &str,
    mut object_exists: impl FnMut(&str) -> bool,
    mut fetch_base_branch: impl FnMut() -> anyhow::Result<()>,
    mut fetch_oid: impl FnMut(&str) -> anyhow::Result<()>,
    branch_tip: impl FnOnce() -> anyhow::Result<String>,
) -> anyhow::Result<(String, bool)> {
    if object_exists(base_ref_oid) {
        return Ok((base_ref_oid.to_string(), false));
    }

    fetch_base_branch()?;
    if object_exists(base_ref_oid) {
        return Ok((base_ref_oid.to_string(), false));
    }

    if fetch_oid(base_ref_oid).is_ok() && object_exists(base_ref_oid) {
        return Ok((base_ref_oid.to_string(), false));
    }

    Ok((branch_tip()?, true))
}

/// Runs `git cat-file -e <oid>^{commit}` in `cwd`, i.e. whether `oid`
/// already exists locally as a commit object — the cheap first check in
/// `resolve_pr_base_sha`'s cascade, run before attempting any fetch.
fn object_exists_locally(cwd: Option<&std::path::Path>, oid: &str) -> bool {
    let mut command = std::process::Command::new("git");
    command.args(["cat-file", "-e", &format!("{oid}^{{commit}}")]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.output().is_ok_and(|output| output.status.success())
}

/// Runs `git fetch origin <oid>` in `cwd` — the direct-oid step of
/// `resolve_pr_base_sha`'s cascade, tried only when the base branch itself
/// (already fetched by the caller) didn't bring the commit in, e.g. after
/// a force-push past it. Unlike `run_git_fetch`, this doesn't need
/// `FETCH_HEAD` afterwards: the caller re-checks `object_exists_locally`
/// instead, since fetching a bare oid doesn't update any ref.
fn fetch_oid(cwd: Option<&std::path::Path>, oid: &str) -> anyhow::Result<()> {
    let mut command = std::process::Command::new("git");
    command.args(["fetch", "origin", oid]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git fetch origin {oid} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Runs `git fetch origin <refspec>` then `git rev-parse FETCH_HEAD` in
/// the repository at `cwd`, returning the resulting SHA. Shared by
/// `fetch_pr_head` and `fetch_branch_head`, which differ only in what
/// refspec they fetch.
///
/// `cwd` selects the repository to run `git` in; `None` uses the
/// process's current directory (production cwd-clone callers),
/// `Some(dir)` pins it (cache clones, tests) — same rationale as
/// `read_git_show_file`'s `cwd`.
fn run_git_fetch(refspec: &str, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
    let mut fetch_command = std::process::Command::new("git");
    fetch_command.args(["fetch", "origin", refspec]);
    if let Some(cwd) = cwd {
        fetch_command.current_dir(cwd);
    }
    let fetch_output = fetch_command.output()?;
    if !fetch_output.status.success() {
        anyhow::bail!(
            "git fetch origin {refspec} failed: {}",
            String::from_utf8_lossy(&fetch_output.stderr)
        );
    }

    let mut rev_parse_command = std::process::Command::new("git");
    rev_parse_command.args(["rev-parse", "FETCH_HEAD"]);
    if let Some(cwd) = cwd {
        rev_parse_command.current_dir(cwd);
    }
    let rev_parse_output = rev_parse_command.output()?;
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

/// Runs `git remote get-url origin` in `cwd` (or the process's current
/// directory when `None`) and returns its stdout, trimmed. `Ok(None)`
/// (rather than an `Err`) when the command fails — not being inside a git
/// repository, or a repository with no `origin` remote, are both
/// expected, ordinary situations for `--pr` URL mode (ADR 0005): they
/// simply mean "the current directory doesn't match, use the cache"
/// rather than a fatal error worth surfacing to the user.
fn git_remote_origin_url(cwd: Option<&std::path::Path>) -> anyhow::Result<Option<String>> {
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

/// Clones `owner/repo` as a blobless partial clone (`--filter=blob:none`,
/// ADR 0005) into `dir` via `gh repo clone`, delegating authentication to
/// `gh` (ADR 0004's stance, applied to cloning too). Only called when
/// `dir` does not already exist — an existing cache entry is refreshed by
/// the ordinary `git fetch` calls in `main` instead of being re-cloned.
fn clone_repo_into_cache(owner: &str, repo: &str, dir: &std::path::Path) -> anyhow::Result<()> {
    let slug = format!("{owner}/{repo}");
    let output = std::process::Command::new("gh")
        .args([
            "repo",
            "clone",
            &slug,
            &dir.to_string_lossy(),
            "--",
            "--filter=blob:none",
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "gh repo clone {slug} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Parses `ghq list --full-path --exact <owner>/<repo>`'s stdout into
/// candidate clone paths: one per non-blank line, trimmed. `ghq` prints
/// one absolute path per line and nothing else on success, but blank
/// lines (a trailing newline, or possibly a stray empty line) are
/// filtered out defensively rather than turned into a bogus empty-path
/// candidate.
fn parse_ghq_list_output(stdout: &str) -> Vec<std::path::PathBuf> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect()
}

/// Runs `ghq list --full-path --exact <owner>/<repo>` and returns the
/// candidate clone paths it reports (ADR 0006's ghq-discovery probe,
/// step 2 between the cwd check and the cache fallback).
///
/// Always returns an empty `Vec` rather than an `Err` — never a fatal
/// error for `--pr` mode — for every way this can fail to find a usable
/// clone: `ghq` missing from `PATH` (`Command::output` fails with
/// `io::ErrorKind::NotFound`), a non-zero exit (e.g. `ghq` installed but
/// its own config is broken), or a zero exit with no matching clones.
/// ADR 0006 is explicit that all of these "fall through silently to the
/// cache"; a `log::debug!` records which case fired, for anyone who wants
/// to know why a clone wasn't discovered without it being an error.
fn ghq_candidate_clones(owner: &str, repo: &str) -> Vec<std::path::PathBuf> {
    let slug = format!("{owner}/{repo}");
    let output = match std::process::Command::new("ghq")
        .args(["list", "--full-path", "--exact", &slug])
        .output()
    {
        Ok(output) => output,
        Err(source) => {
            log::debug!("ghq not runnable, falling back to cache for {slug}: {source}");
            return Vec::new();
        }
    };
    if !output.status.success() {
        log::debug!(
            "ghq list {slug} exited non-zero, falling back to cache: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Vec::new();
    }
    match String::from_utf8(output.stdout) {
        Ok(stdout) => parse_ghq_list_output(&stdout),
        Err(source) => {
            log::debug!(
                "ghq list {slug} produced non-UTF-8 output, falling back to cache: {source}"
            );
            Vec::new()
        }
    }
}

/// Picks the first of `candidates` whose origin (resolved by the injected
/// `origin_of` port) matches `owner`/`repo` per `github_remote_matches`.
/// `None` if `candidates` is empty or none of them match.
///
/// `origin_of` is injected — production wires it to
/// `|path| git_remote_origin_url(Some(path)).ok().flatten()`, tests wire
/// it to an in-memory map — so this selection logic is unit-testable
/// without shelling out to `git`, following the same read-file-port style
/// as `analyze_diff`/`build_resolver` elsewhere in this file.
fn select_matching_clone(
    candidates: &[std::path::PathBuf],
    origin_of: impl Fn(&std::path::Path) -> Option<String>,
    owner: &str,
    repo: &str,
) -> Option<std::path::PathBuf> {
    candidates
        .iter()
        .find(|candidate| {
            origin_of(candidate).is_some_and(|origin| github_remote_matches(&origin, owner, repo))
        })
        .cloned()
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
    #[case::should_parse_bare_number("76", PrArg::Number(76))]
    #[case::should_parse_number_with_surrounding_whitespace(" 76 ", PrArg::Number(76))]
    #[case::should_parse_pull_url(
        "https://github.com/octocat/hello-world/pull/123",
        PrArg::Url {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            number: 123,
        }
    )]
    #[case::should_parse_pull_url_with_trailing_slash(
        "https://github.com/octocat/hello-world/pull/123/",
        PrArg::Url {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            number: 123,
        }
    )]
    #[case::should_parse_pull_url_with_extra_path_segment(
        "https://github.com/octocat/hello-world/pull/123/files",
        PrArg::Url {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            number: 123,
        }
    )]
    fn should_parse_pr_arg_when_input_is_valid(#[case] input: &str, #[case] expected: PrArg) {
        let actual = parse_pr_arg(input).expect("expected a valid PR arg");

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

    #[rstest]
    #[case::should_prefer_rinkaku_cache_dir_when_set(
        Some("/custom/cache"),
        Some("/xdg/cache"),
        Some("/home/user"),
        "/custom/cache/repos/github.com/octocat/hello-world"
    )]
    #[case::should_fall_back_to_xdg_cache_home_when_rinkaku_cache_dir_unset(
        None,
        Some("/xdg/cache"),
        Some("/home/user"),
        "/xdg/cache/rinkaku/repos/github.com/octocat/hello-world"
    )]
    #[case::should_fall_back_to_home_when_neither_env_var_set(
        None,
        None,
        Some("/home/user"),
        "/home/user/.cache/rinkaku/repos/github.com/octocat/hello-world"
    )]
    fn should_build_cache_repo_dir(
        #[case] rinkaku_cache_dir: Option<&str>,
        #[case] xdg_cache_home: Option<&str>,
        #[case] home: Option<&str>,
        #[case] expected: &str,
    ) {
        let actual = cache_repo_dir(
            rinkaku_cache_dir,
            xdg_cache_home,
            home,
            "octocat",
            "hello-world",
        )
        .expect("expected a cache directory to be resolved");

        assert_eq!(std::path::PathBuf::from(expected), actual);
    }

    #[test]
    fn should_fail_to_build_cache_repo_dir_when_no_env_source_is_available() {
        let actual = cache_repo_dir(None, None, None, "octocat", "hello-world");

        assert!(actual.is_err());
    }

    #[rstest]
    #[case::should_parse_single_line(
        "/home/user/ghq/github.com/octocat/hello-world\n",
        vec![std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world")]
    )]
    #[case::should_parse_multiple_lines(
        "/home/user/ghq/github.com/octocat/hello-world\n/home/user/work/hello-world\n",
        vec![
            std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world"),
            std::path::PathBuf::from("/home/user/work/hello-world"),
        ]
    )]
    #[case::should_skip_blank_lines_between_entries(
        "/home/user/ghq/github.com/octocat/hello-world\n\n/home/user/work/hello-world\n",
        vec![
            std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world"),
            std::path::PathBuf::from("/home/user/work/hello-world"),
        ]
    )]
    #[case::should_trim_surrounding_whitespace_per_line(
        "  /home/user/ghq/github.com/octocat/hello-world  \n",
        vec![std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world")]
    )]
    #[case::should_return_empty_vec_for_empty_string("", vec![])]
    #[case::should_return_empty_vec_for_whitespace_only_string("\n\n  \n", vec![])]
    fn should_parse_ghq_list_output(
        #[case] stdout: &str,
        #[case] expected: Vec<std::path::PathBuf>,
    ) {
        let actual = parse_ghq_list_output(stdout);

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_return_first_candidate_when_it_matches(
        vec!["/a", "/b"],
        vec![("/a", "https://github.com/octocat/hello-world.git")],
        Some(std::path::PathBuf::from("/a"))
    )]
    #[case::should_return_later_candidate_when_earlier_ones_mismatch(
        vec!["/a", "/b", "/c"],
        vec![
            ("/a", "https://github.com/someone-else/other-repo.git"),
            ("/b", "https://github.com/octocat/hello-world.git"),
        ],
        Some(std::path::PathBuf::from("/b"))
    )]
    #[case::should_return_none_when_no_candidate_matches(
        vec!["/a", "/b"],
        vec![
            ("/a", "https://github.com/someone-else/other-repo.git"),
            ("/b", "https://gitlab.com/octocat/hello-world.git"),
        ],
        None
    )]
    #[case::should_return_none_when_candidates_is_empty(vec![], vec![], None)]
    #[case::should_return_none_when_origin_lookup_yields_nothing_for_any_candidate(
        vec!["/a"],
        vec![],
        None
    )]
    fn should_select_matching_clone(
        #[case] candidates: Vec<&str>,
        #[case] origins: Vec<(&str, &str)>,
        #[case] expected: Option<std::path::PathBuf>,
    ) {
        let candidates: Vec<std::path::PathBuf> = candidates
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        let origin_of = |path: &std::path::Path| {
            origins
                .iter()
                .find(|(candidate_path, _)| std::path::Path::new(candidate_path) == path)
                .map(|(_, origin)| origin.to_string())
        };

        let actual = select_matching_clone(&candidates, origin_of, "octocat", "hello-world");

        assert_eq!(expected, actual);
    }

    mod resolve_pr_base_sha_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use std::cell::RefCell;

        #[test]
        fn should_return_base_ref_oid_when_it_already_exists_locally() {
            let fetch_base_branch_calls = RefCell::new(0);
            let fetch_oid_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| true,
                || {
                    *fetch_base_branch_calls.borrow_mut() += 1;
                    Ok(())
                },
                |_oid| {
                    *fetch_oid_calls.borrow_mut() += 1;
                    Ok(())
                },
                || panic!("branch_tip must not be called when base_ref_oid already exists"),
            )
            .expect("should resolve without error");

            assert_eq!(("base789".to_string(), false), actual);
            assert_eq!(0, *fetch_base_branch_calls.borrow());
            assert_eq!(0, *fetch_oid_calls.borrow());
        }

        #[test]
        fn should_return_base_ref_oid_when_fetching_the_base_branch_makes_it_available() {
            let exists_calls = RefCell::new(0);
            let object_exists = |_oid: &str| {
                let mut calls = exists_calls.borrow_mut();
                *calls += 1;
                // First check (before any fetch) fails; the check right
                // after `fetch_base_branch` succeeds.
                *calls > 1
            };

            let actual = resolve_pr_base_sha(
                "base789",
                object_exists,
                || Ok(()),
                |_oid| panic!("fetch_oid must not be called when the base branch fetch sufficed"),
                || panic!("branch_tip must not be called when the base branch fetch sufficed"),
            )
            .expect("should resolve without error");

            assert_eq!(("base789".to_string(), false), actual);
        }

        #[test]
        fn should_return_base_ref_oid_when_fetching_the_oid_directly_makes_it_available() {
            let exists_calls = RefCell::new(0);
            let object_exists = |_oid: &str| {
                let mut calls = exists_calls.borrow_mut();
                *calls += 1;
                // Neither the initial check nor the one after the base
                // branch fetch succeed; only the one after `fetch_oid`
                // does (third call).
                *calls > 2
            };

            let actual = resolve_pr_base_sha(
                "base789",
                object_exists,
                || Ok(()),
                |_oid| Ok(()),
                || panic!("branch_tip must not be called when fetch_oid sufficed"),
            )
            .expect("should resolve without error");

            assert_eq!(("base789".to_string(), false), actual);
        }

        #[test]
        fn should_fall_back_to_branch_tip_when_the_oid_is_unreachable_by_any_means() {
            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || Ok(()),
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
                || Ok("branch-tip-sha".to_string()),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
        }

        #[test]
        fn should_fall_back_to_branch_tip_when_fetch_oid_succeeds_but_object_still_missing() {
            // `git fetch origin <oid>` can itself succeed (e.g. the remote
            // accepts the request) while the object is still not resolvable
            // locally afterwards — covered separately from the "fetch_oid
            // errors outright" case above.
            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || Ok(()),
                |_oid| Ok(()),
                || Ok("branch-tip-sha".to_string()),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
        }

        #[test]
        fn should_propagate_error_when_fetching_the_base_branch_fails() {
            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || anyhow::bail!("simulated: git fetch origin main failed"),
                |_oid| Ok(()),
                || Ok("branch-tip-sha".to_string()),
            );

            assert!(actual.is_err());
        }

        #[test]
        fn should_propagate_error_when_the_branch_tip_fallback_itself_fails() {
            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || Ok(()),
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
                || anyhow::bail!("simulated: git fetch origin main failed"),
            );

            assert!(actual.is_err());
        }
    }

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
