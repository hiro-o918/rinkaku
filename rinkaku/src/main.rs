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
mod spinner;

use clap::{Parser, Subcommand};
use rinkaku_core::deps::TagsResolver;
use rinkaku_core::language::language_for_path;
use rinkaku_core::pipeline::analyze_diff;
use rinkaku_core::render::{OutputFormat, render};
use spinner::{AnalysisPhase, Spinner, phase_message};
use std::io::BufRead;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;

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

    /// Output format. Defaults to Markdown, or the interactive TUI when
    /// stdout is a terminal and neither `--format` nor `--tui` was given
    /// (ADR 0017) — see `resolve_display_mode`.
    //
    // `Option` rather than a `default_value_t` is what makes "the user
    // didn't pass --format" observable at all; a defaulted `Format` field
    // would look identical to an explicit `--format md`, which
    // `resolve_display_mode` needs to tell apart (see its own doc comment).
    #[arg(long, value_enum, conflicts_with = "tui")]
    format: Option<Format>,

    /// Open the interactive terminal UI (ADR 0015/0016) instead of
    /// printing Markdown/JSON. The input flow (stdin / `--base` / `--pr`)
    /// is unchanged — `--tui` only changes the output stage, once a
    /// `Report` is built. Conflicts with `--format`, since the two are
    /// mutually exclusive output stages rather than combinable options.
    #[arg(long, default_value_t = false)]
    tui: bool,

    /// Whether to resolve each changed symbol's 1-hop dependencies
    /// (ADR 0003). `1` (default) runs the tags-based `Resolver` over
    /// every file tracked by `git ls-files`; `0` skips resolution
    /// entirely (no `Resolver::resolve` calls), which is faster and
    /// avoids the repo-wide indexing pass.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=1))]
    deps: u8,

    /// Exclude test symbols from the "Change graph"/"Definitions" output
    /// and summarize their per-file counts under a "Tests" section
    /// instead (ADR 0025, superseding the ADR 0009 default). Without
    /// this flag, test symbols appear in the graph and definitions like
    /// any other symbol — the default the Markdown/JSON output is
    /// designed around now that its primary audience is LLM reviewers
    /// (humans read the TUI, which badges test files rather than
    /// omitting them).
    #[arg(long, default_value_t = false)]
    exclude_tests: bool,

    /// Include files `.gitattributes` marks `-diff` or `linguist-generated`
    /// instead of skipping them by default (ADR 0010).
    #[arg(long, default_value_t = false)]
    include_generated: bool,

    /// Re-root the change graph at this path before rendering (ADR 0019):
    /// entry points become the symbols under `path` that nothing else
    /// under that same path depends on, and dependency trees still expand
    /// outward through the full graph as usual. This is a viewpoint change,
    /// not a filter — symbols outside `path` are neither hidden nor
    /// excluded from analysis, only no longer eligible to be roots
    /// themselves. Compatible with every input mode (stdin/`--base`/`--pr`/
    /// whole-repo) and with `--tui`: combined, the TUI opens with the
    /// cursor already on the tree row matching `path` and the right pane
    /// already showing its Blast radius (`rinkaku_tui::run`'s `entry_path`
    /// parameter; ADR 0023), rather than requiring the reviewer to find the
    /// row and press `R` themselves.
    #[arg(long)]
    entry: Option<String>,
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
    /// A human-oriented call/dependency graph as a mermaid `flowchart`
    /// document (ADR 0021) — opt-in, aimed at GitHub's native mermaid
    /// rendering in PR comments/descriptions, not the default Markdown
    /// output.
    Mermaid,
}

impl From<Format> for OutputFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Md => OutputFormat::Markdown,
            Format::Json => OutputFormat::Json,
            Format::Mermaid => OutputFormat::Mermaid,
        }
    }
}

fn main() -> anyhow::Result<()> {
    // Default to `info`-level progress output on stderr (env_logger's own
    // default is error-only, which meant `--pr`/`--base` runs — the ones
    // slow enough to want a heartbeat, see the dependency-index build
    // below — gave no feedback at all while running). `RUST_LOG` still
    // overrides this, same as any other `env_logger::Builder::from_env`
    // setup.
    //
    // Timestamp and module path are dropped: this is a short-lived
    // one-shot CLI, so there is nothing to correlate a timestamp against,
    // and the binary is a single crate, making the module path redundant.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .format_target(false)
        .init();
    let cli = Cli::parse();

    if let Some(Command::SelfUpdate { yes }) = cli.command {
        return self_update::run_self_update(yes);
    }

    // Tracks the same `cwd`/`workdir` each branch below already resolves for
    // its own `git`/`gh` calls, so the TUI's source view (`repo_root`,
    // below) reads files from the repository the `Report` was actually
    // built from rather than always the process's current directory —
    // `--pr` in particular can run entirely against a ghq/cache clone
    // elsewhere on disk (`resolve_pr_workdir`), and `resolve_repo_root(None)`
    // would silently resolve the *process's* repo instead, showing an
    // unrelated file if one happens to exist at the same relative path
    // there.
    let mut resolved_workdir: Option<std::path::PathBuf> = None;
    // Started before any branch below runs and cleared right after the
    // pipeline finishes (`spinner.finish_and_clear()`), so the whole
    // synchronous analysis phase — the only part of a run with no
    // per-symbol feedback of its own — gets a visible heartbeat on stderr.
    // `Spinner::start` is a no-op-looking wrapper around `indicatif`, whose
    // stderr draw target already suppresses drawing when stderr isn't a
    // terminal (see `spinner.rs`'s own doc comment), so this is safe to run
    // unconditionally in every input mode, including piped stderr.
    let spinner = Spinner::start(phase_message(AnalysisPhase::Starting));
    let (report, diff_text) = if let Some(pr_arg) = &cli.pr {
        // Validate the arg and derive the fetch refspec's PR number, but
        // pass the original (trimmed) value — not the parsed number — to
        // `gh pr view` (see that function's doc comment for why).
        let parsed = parse_pr_arg(pr_arg)?;
        let number = parsed.number();
        spinner.set_message(phase_message(AnalysisPhase::ResolvingPr));
        let workdir = resolve_pr_workdir(&parsed)?;
        resolved_workdir = workdir.clone();
        log::info!("resolving PR #{number} via gh");
        let pr_info = fetch_pr_info(pr_arg.trim())?;
        let cwd = workdir.as_deref();
        log::info!("fetching PR #{number} head");
        let head_sha = fetch_pr_head(number, cwd)?;
        if head_sha != pr_info.head_ref_oid {
            anyhow::bail!(
                "fetched PR #{number} head ({head_sha}) does not match `gh`'s reported head \
                 ({expected}); this usually means the PR belongs to a different repository than \
                 the target clone's `origin` remote, or the PR was updated between resolving it \
                 and fetching it — verify `origin` points at the PR's repository and re-run",
                expected = pr_info.head_ref_oid,
            );
        }
        log::info!("resolving PR #{number} base commit");
        let (base_sha, used_fallback) = resolve_pr_base_sha(
            &pr_info.base_ref_oid,
            |oid| object_exists_locally(cwd, oid),
            || fetch_branch_head(&pr_info.base_ref_name, cwd),
            |oid| fetch_oid(cwd, oid),
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
        run_base_pipeline(&cli, &base_sha, &head_sha, cwd, &spinner)?
    } else if let Some(base) = &cli.base {
        run_base_pipeline(&cli, base, &cli.head, None, &spinner)?
    } else if std::io::stdin().is_terminal() {
        // ADR 0017: this is the third arm of an `if let Some(pr) ... else if
        // let Some(base) ... else if <here>` chain, so reaching it already
        // means `cli.pr` and `cli.base` are both `None` — no need to check
        // again. With no `--base`/`--pr` and stdin attached to a terminal,
        // there is no diff to read at all, so a whole-repo outline is built
        // instead of falling through to `read_stdin_diff`'s "no diff input"
        // error. `diff_text` is empty: the TUI's diff pane (`d`) has nothing
        // to slice hunks out of in this mode and falls back to its
        // placeholder (ADR 0017's Consequences).
        //
        // `read_stdin_diff`'s own `is_terminal()` bail is unreachable via
        // this chain today (every stdin-is-a-TTY case is caught here first),
        // but is kept as a defensive check in case this `if`/`else if` chain
        // is ever restructured — e.g. a future flag added between this arm
        // and the plain stdin-read fallback below.
        log::info!("no diff input and stdin is a terminal; building a whole-repo outline");
        spinner.set_message(phase_message(AnalysisPhase::ParsingRepository));
        let paths = list_repo_files_for_outline(None)?;
        // `check_generated_paths_batch`, not `resolve_generated_paths`
        // (which shells out via `check_generated_paths`'s CLI-argument
        // form): `paths` here is every tracked file, potentially far more
        // than a diff's changed-path count, and passing thousands of paths
        // as CLI arguments risks exceeding the OS's `ARG_MAX` — the same
        // reason `build_resolver` already uses the batch/stdin form for
        // its own repo-wide scan (see that function's doc comment).
        let generated_paths = if cli.include_generated {
            std::collections::HashSet::new()
        } else {
            check_generated_paths_batch(None, &paths)
        };
        let report = rinkaku_core::pipeline::analyze_repo(
            &paths,
            read_working_tree_file,
            // Core's `include_tests: bool` keeps its original meaning
            // ("true means include tests"). Only the CLI-side polarity is
            // flipped by ADR 0025, so translate here.
            !cli.exclude_tests,
            &generated_paths,
            cli.include_generated,
        );
        if let Some(note) = repo_outline_empty_note(&report) {
            eprintln!("{note}");
        }
        (report, String::new())
    } else {
        let diff_text = read_stdin_diff()?;
        if diff_text.trim().is_empty() {
            eprintln!("note: diff is empty, nothing to analyze");
        }
        let resolver = build_resolver(
            &cli,
            &diff_text,
            read_working_tree_file,
            None,
            None,
            &spinner,
        )?;
        let changed_paths = changed_paths(&diff_text)?;
        let generated_paths = resolve_generated_paths(&cli, &changed_paths, None);
        log::info!("analyzing diff");
        spinner.set_message(phase_message(AnalysisPhase::AnalyzingDiff));
        let report = analyze_diff(
            &diff_text,
            read_working_tree_file,
            // Pure stdin-pipe input has no known base commit (see this
            // module's own doc comment on stdin mode), so ADR 0014's
            // classification stays unknown for every symbol rather than
            // guessing one from partial information.
            None,
            resolver
                .as_ref()
                .map(|r| r as &dyn rinkaku_core::deps::Resolver),
            // Same translation as the `analyze_repo` call above: core's
            // `include_tests` is the semantic name, ADR 0025 flips only
            // the CLI-facing polarity.
            !cli.exclude_tests,
            &generated_paths,
            cli.include_generated,
        )?;
        if let Some(note) = garbage_input_note(&diff_text, &report) {
            eprintln!("{note}");
        }
        (report, diff_text)
    };
    // Cleared as soon as the `Report` is built, before the `--entry` pivot
    // (pure/instant) and the display-mode dispatch below — in particular
    // before `DisplayMode::Tui` enters the alternate screen, since a
    // spinner line still drawn on stderr at that point would corrupt the
    // TUI's first frame (`spinner.rs`'s own doc comment).
    spinner.finish_and_clear();

    let report = if let Some(entry) = &cli.entry {
        let pivoted = apply_entry_pivot(report, entry);
        if let Some(note) = entry_pivot_empty_note(&pivoted, entry) {
            eprintln!("{note}");
        }
        pivoted
    } else {
        report
    };

    let stdout_is_tty = std::io::stdout().is_terminal();
    match resolve_display_mode(cli.tui, cli.format, stdout_is_tty) {
        DisplayMode::Tui => {
            let repo_root = resolve_repo_root(resolved_workdir.as_deref());
            rinkaku_tui::run(&report, &diff_text, cli.entry.as_deref(), &repo_root)?
        }
        DisplayMode::Output(format) => {
            let output = render(&report, format.into())?;
            print!("{output}");
        }
    }

    Ok(())
}

/// Applies `--entry <path>` (ADR 0019) to an already-built `Report`: swaps
/// `report.graph` for `graph::pivot_graph`'s re-rooted clone, leaving every
/// other field (`files`, `hotspots`, `removed`, ...) untouched — the pivot
/// only changes which nodes `render`/`rinkaku-tui` treat as entry points,
/// not what was analyzed.
fn apply_entry_pivot(
    report: rinkaku_core::render::Report,
    path: &str,
) -> rinkaku_core::render::Report {
    let graph = rinkaku_core::graph::pivot_graph(&report.graph, path);
    rinkaku_core::render::Report { graph, ..report }
}

/// Returns a note for `--entry <path>` (ADR 0019) when no symbol's path
/// falls under `path` at all — mirroring `garbage_input_note`/
/// `repo_outline_empty_note`'s existing pure-note-then-`eprintln!`-at-the-
/// call-site pattern rather than having `apply_entry_pivot` itself perform
/// IO. `None` when the report had no symbols to begin with (an empty graph
/// pivoting to an empty graph is not a pivot-specific problem worth a
/// separate note — `garbage_input_note`/`repo_outline_empty_note` already
/// cover that case for their respective input modes).
///
/// Takes the *already-pivoted* `report` (i.e. `apply_entry_pivot`'s own
/// output) rather than re-running `graph::pivot_roots` itself: the call
/// site used to run `pivot_roots` here and then `pivot_graph` (which
/// internally calls `pivot_roots` again) in `apply_entry_pivot`, computing
/// the same root set twice. `graph.roots` on the pivoted report already
/// *is* that root set (`pivot_graph`'s own doc comment), and `graph.nodes`
/// is untouched by pivoting either way, so checking `nodes.is_empty()` for
/// the "no symbols at all" case is equally valid before or after.
fn entry_pivot_empty_note(report: &rinkaku_core::render::Report, path: &str) -> Option<String> {
    if report.graph.nodes.is_empty() {
        return None;
    }
    if report.graph.roots.is_empty() {
        Some(format!("note: no symbols under {path}"))
    } else {
        None
    }
}

/// Which output stage `main` dispatches to, once a `Report` is built —
/// pulled into its own type (rather than inlining the `if cli.tui`/
/// `render` branch as before) so the *decision* of which one to use can be
/// unit-tested as a pure function ([`resolve_display_mode`]) independent
/// of actually running the TUI or rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayMode {
    Tui,
    Output(Format),
}

/// Decides which [`DisplayMode`] to use from the three inputs that can
/// influence it: whether `--tui` was passed, whether `--format` was passed
/// (`Some` — clap's `conflicts_with` already guarantees `tui` and `format`
/// are never both meaningfully set, see `Cli::format`'s doc comment), and
/// whether stdout is a terminal.
///
/// - `--tui` passed → [`DisplayMode::Tui`], regardless of stdout.
/// - `--format` passed (and `--tui` wasn't, by the conflict above) →
///   [`DisplayMode::Output`] with that format — an explicit format request
///   always wins, whether or not stdout happens to be a terminal (this is
///   what lets a non-interactive caller force whole-repo mode's Markdown
///   output even while attached to a terminal, e.g. `rinkaku --format md
///   > out.md` run interactively, or this project's own dogfooding
///   `rinkaku --format md` invocations in CI-like scripts).
/// - Neither passed → ADR 0017's default: [`DisplayMode::Tui`] when stdout
///   is a terminal (a human is watching, so they get the interactive
///   view — ADR 0015), [`DisplayMode::Output(Format::Md)`] otherwise (a
///   pipe/redirect, so Markdown is what a non-interactive consumer can
///   actually use).
///
/// Pure and total over its three `bool`/`Option` inputs — no `IsTerminal`
/// call here, `main` reads the real streams and passes the results in.
fn resolve_display_mode(tui: bool, format: Option<Format>, stdout_is_tty: bool) -> DisplayMode {
    if tui {
        return DisplayMode::Tui;
    }
    if let Some(format) = format {
        return DisplayMode::Output(format);
    }
    if stdout_is_tty {
        DisplayMode::Tui
    } else {
        DisplayMode::Output(Format::Md)
    }
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
        log::info!("using the current directory as a clone of {owner}/{repo}");
        return Ok(None);
    }

    let ghq_candidates = ghq_candidate_clones(owner, repo);
    if let Some(discovered) = select_matching_clone(
        &ghq_candidates,
        |path| git_remote_origin_url(Some(path)).ok().flatten(),
        owner,
        repo,
    ) {
        log::info!(
            "using ghq-managed clone of {owner}/{repo} at {}",
            discovered.display()
        );
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
        log::info!(
            "cloning {owner}/{repo} into cache at {} (first run against this repository)",
            dir.display()
        );
        clone_repo_into_cache(owner, repo, &dir)?;
    } else {
        log::info!("using cache clone of {owner}/{repo} at {}", dir.display());
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
/// An empty (or whitespace-only) diff — most commonly a `--base`/`--pr`
/// range with no actual changes, e.g. `base == head` — returns the empty
/// `Report` immediately, printing the same "diff is empty" note the stdin
/// path prints, and **without calling `build_resolver`**: indexing every
/// tracked file for dependency resolution is pointless work when there is
/// nothing to resolve dependencies for, and on a large repository it is
/// also the single slowest part of a run (one `git show`/`cat-file` per
/// tracked file). A non-empty diff that nonetheless yields zero entries
/// (garbage input) still gets its own note via `garbage_input_note` after
/// the full pipeline runs, same as stdin mode.
///
/// `cwd` selects which repository every subprocess in this call runs in
/// (same rationale as `read_git_show_file`'s `cwd`: `None` uses the
/// process's current directory for `--base` and cwd-clone `--pr` runs,
/// `Some(dir)` targets a cache clone (ADR 0005) or a test fixture).
///
/// Returns the raw diff text alongside the `Report` (TUI iteration 2): the
/// `--tui` diff pane needs the same unified diff `analyze_diff` was built
/// from to slice hunks out of, and this is the only place that owns it for
/// `--base`/`--pr` mode — `main`'s stdin branch already has `diff_text` in
/// a local variable, so it needs no such plumbing.
///
/// `spinner` updates its message as each phase (diffing, then building the
/// dependency index via `build_resolver`, then the diff analysis itself)
/// starts, so the pre-TUI stderr spinner (`main`'s `Spinner::start`) tracks
/// which phase is running rather than sitting on a single static message
/// for the whole `--base`/`--pr` pipeline.
fn run_base_pipeline(
    cli: &Cli,
    base: &str,
    head: &str,
    cwd: Option<&std::path::Path>,
    spinner: &Spinner,
) -> anyhow::Result<(rinkaku_core::render::Report, String)> {
    log::info!("diffing {base}...{head}");
    spinner.set_message(phase_message(AnalysisPhase::Diffing));
    let diff_text = run_git_diff(base, head, cwd)?;
    if diff_text.trim().is_empty() {
        eprintln!("note: diff is empty, nothing to analyze");
        return Ok((
            rinkaku_core::render::Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: Vec::new(),
                skipped: Vec::new(),
                graph: rinkaku_core::graph::SymbolGraph {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    roots: Vec::new(),
                },
                tests: Vec::new(),
                hotspots: Vec::new(),
                file_size_warnings: Vec::new(),
                removed: Vec::new(),
            },
            diff_text,
        ));
    }

    let read_file = {
        let head = head.to_string();
        move |path: &str| read_git_show_file(cwd, &head, path)
    };
    // ADR 0014: `--base`/`--pr` mode always knows a base commit, so unlike
    // stdin mode (see `main`'s own `analyze_diff` call), a `read_base_file`
    // port is always supplied here rather than `None` — reusing the same
    // `git show <rev>:<path>` strategy `read_file` already uses for the
    // head side, just pointed at `base` instead. A path that doesn't exist
    // on the base side (e.g. a brand-new file) fails this read, which
    // `analyze_diff` treats as "no base content for this file" rather than
    // an error (see its own doc comment).
    let read_base_file = {
        let base = base.to_string();
        move |path: &str| read_git_show_file(cwd, &base, path)
    };
    let resolver = build_resolver(cli, &diff_text, &read_file, Some(head), cwd, spinner)?;
    let changed_paths = changed_paths(&diff_text)?;
    let generated_paths = resolve_generated_paths(cli, &changed_paths, cwd);
    log::info!("analyzing diff");
    spinner.set_message(phase_message(AnalysisPhase::AnalyzingDiff));
    let report = analyze_diff(
        &diff_text,
        read_file,
        Some(&read_base_file),
        resolver
            .as_ref()
            .map(|r| r as &dyn rinkaku_core::deps::Resolver),
        // See sibling `analyze_diff` call in `main` for why this negates
        // `exclude_tests` rather than passing it straight through.
        !cli.exclude_tests,
        &generated_paths,
        cli.include_generated,
    )?;
    if let Some(note) = garbage_input_note(&diff_text, &report) {
        eprintln!("{note}");
    }
    Ok((report, diff_text))
}

/// Parses `diff_text` and extracts just the changed paths, for callers that
/// only need path strings (currently only `resolve_generated_paths`) rather
/// than the full `ChangedFile` data `analyze_diff` itself parses out.
/// Sharing this single parse between `main`/`run_base_pipeline` and
/// `resolve_generated_paths` is what lets `resolve_generated_paths` stay
/// diff-text-free (see its own doc comment) — this function is the one
/// place that still pays that specific parse's cost, once per run.
fn changed_paths(diff_text: &str) -> anyhow::Result<Vec<String>> {
    Ok(rinkaku_core::diff::parse_unified_diff(diff_text)?
        .into_iter()
        .map(|changed_file| changed_file.path)
        .collect())
}

/// Resolves ADR 0010's generated-path set for `changed_paths`, or an empty
/// set when `cli.include_generated` opts out.
///
/// Takes already-parsed changed paths rather than the raw diff text, so it
/// never parses the diff itself: `main`/`run_base_pipeline` parse
/// `diff_text` via `parse_unified_diff` exactly once per run and pass the
/// resulting paths here, instead of this function re-parsing the same text
/// a third time on top of `analyze_diff`'s own parse and
/// `build_resolver`'s `collect_referenced_names` parse (both still
/// separate, accepted the same way `collect_referenced_names`'s own doc
/// comment already accepts that double-parse). Infallible now that there is
/// no parse step of its own to fail — `check_generated_paths` never
/// returns an `Err` either (see its own doc comment).
///
/// `cwd` is passed straight through to `check_generated_paths`, so it
/// inherits that function's best-effort behavior: no local repository (or
/// `git check-attr` failing for any reason) yields an empty set rather than
/// an error, since attribute filtering is a nice-to-have on top of the
/// primary diff-condensation flow, not a hard requirement of it (ADR 0010).
fn resolve_generated_paths(
    cli: &Cli,
    changed_paths: &[String],
    cwd: Option<&std::path::Path>,
) -> std::collections::HashSet<String> {
    if cli.include_generated {
        return std::collections::HashSet::new();
    }
    check_generated_paths(cwd, changed_paths)
}

/// Returns a warning note for stdin input that is garbage rather than a
/// unified diff — non-empty input that nonetheless produced zero
/// recognized file entries (`parse_unified_diff` never errors on
/// unrecognized text, it simply finds nothing to report, see `diff.rs`),
/// which would otherwise silently exit 0 with an empty report and no
/// indication anything went wrong. `None` when `diff_text` is empty or
/// whitespace-only (already covered by the separate "diff is empty" note
/// at the call site — the two notes are mutually exclusive) or when the
/// report has any file, skip, or test-summary entry at all — a diff that
/// touched only test symbols (ADR 0009's default exclusion moves them out
/// of `files` into `tests`) is a fully-recognized, legitimate result, not
/// garbage input, even though `files`/`skipped` are both empty in that
/// case.
fn garbage_input_note(
    diff_text: &str,
    report: &rinkaku_core::render::Report,
) -> Option<&'static str> {
    if diff_text.trim().is_empty() {
        return None;
    }
    if !report.files.is_empty() || !report.skipped.is_empty() || !report.tests.is_empty() {
        return None;
    }
    Some("note: no file changes recognized in input; expected a unified diff")
}

/// Returns a note for ADR 0017's whole-repo outline when it found nothing
/// to show — every tracked file was either unsupported, a whole test file,
/// generated, or unreadable (`analyze_repo`'s own doc comment: all of these
/// are dropped silently, with no `SkippedFile`/`TestFileSummary` entry to
/// record why, unlike diff mode) — so an empty `stdout` would otherwise
/// look identical to "ran fine, nothing to say" with no indication that a
/// git repository with zero recognizable source files is likely a
/// misconfiguration (wrong directory, `.gitignore`-only repo, etc.).
///
/// Unlike `garbage_input_note`, only `files`/`removed` are checked:
/// `analyze_repo` never populates `skipped`/`tests` at all, so those two
/// fields carry no information in this mode to check against.
fn repo_outline_empty_note(report: &rinkaku_core::render::Report) -> Option<&'static str> {
    if !report.files.is_empty() || !report.removed.is_empty() {
        return None;
    }
    Some("note: no supported source files found in the repository")
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
/// content is read via a single `git cat-file --batch` process
/// (`read_git_show_files_batch` — one process for every tracked file,
/// rather than one `git show` subprocess per file) so the index and the
/// diff being analyzed are consistent with the same commit regardless of
/// the working tree's state (same rationale as `read_git_show_file`,
/// applied here to the whole repo rather than just the changed files).
/// `cwd` selects the repository `list_git_files` runs `git ls-files` in
/// (same rationale as `read_git_show_file`'s `cwd`: `None` uses the
/// process's current directory for production callers, `Some(dir)` pins
/// it for tests). Only reached when `cli.deps != 0` — the `deps == 0`
/// branch returns before doing any repository scan at all, verified by
/// `should_skip_git_ls_files_when_deps_is_zero` below (pointing `cwd` at
/// a directory with no git repository would make `list_git_files` fail,
/// so a passing `Ok(None)` there is proof the scan never ran).
///
/// `!cli.exclude_tests` is threaded through to `TagsResolver::new` (ADR
/// 0009's exclusion mechanism, retained under ADR 0025's inverted CLI
/// flag), so the repo-wide index applies the same test-inclusion decision
/// `analyze_diff` uses for the diff's own symbols. With `--exclude-tests`,
/// this stops a changed production symbol's "Depends on:" from resolving
/// to a same-named test helper/fixture elsewhere in the repo (almost
/// always a false match rather than a real dependency). `cli.include_generated` is
/// threaded the same way, alongside a `generated_paths` set resolved for
/// every tracked path via `check_generated_paths_batch` (ADR 0010's
/// `.gitattributes` check, run once over the whole index rather than the
/// diff's changed paths — see that function's doc comment for why it
/// streams paths over stdin instead of passing them as CLI arguments the
/// way `resolve_generated_paths` does for the diff). `TagsResolver::new`
/// additionally runs ADR 0011's content-marker check itself once each
/// file's content is available. Without either exclusion, a changed
/// production symbol's "Depends on:" could just as easily resolve to a
/// generated file's definition (e.g. an ORM's model struct, dragging in
/// every column as noise) as a test helper — see ADR 0010/0011's
/// Consequences.
///
/// `spinner`'s message is updated to reflect the indexing phase once it's
/// clear there is indexing work to do (i.e. after the `cli.deps == 0` early
/// return) — same pre-TUI stderr spinner `main` starts before dispatching
/// to any input-mode branch.
fn build_resolver(
    cli: &Cli,
    diff_text: &str,
    diff_read_file: impl Fn(&str) -> std::io::Result<String>,
    head: Option<&str>,
    cwd: Option<&std::path::Path>,
    spinner: &Spinner,
) -> anyhow::Result<Option<TagsResolver>> {
    if cli.deps == 0 {
        return Ok(None);
    }
    spinner.set_message(phase_message(AnalysisPhase::BuildingDependencyIndex));

    let reference_names =
        rinkaku_core::pipeline::collect_referenced_names(diff_text, diff_read_file)?;

    let paths = list_git_files(cwd)?;
    log::info!(
        "building dependency index over {} tracked files",
        paths.len()
    );
    let generated_paths = if cli.include_generated {
        std::collections::HashSet::new()
    } else {
        check_generated_paths_batch(cwd, &paths)
    };
    let files: Vec<(String, String)> = match head {
        // One `git cat-file --batch` child process serves every path
        // (see `read_git_show_files_batch`'s doc comment for why this
        // replaces a `git show` subprocess per file). A single
        // unresolvable path is isolated inside that call (same
        // best-effort skip as the working-tree branch below); the `?`
        // here only ever fires for a genuinely unrecoverable failure
        // (the child process itself failing to start, or the batch
        // stream desyncing), which cannot be isolated to one path.
        Some(head) => read_git_show_files_batch(cwd, head, paths)?,
        None => paths
            .into_iter()
            .filter_map(|path| {
                // A file listed by `git ls-files` can still fail to read
                // (e.g. deleted in the working tree but not yet staged, a
                // submodule gitlink entry) — skipped rather than failing
                // the whole run, since the resolver's index is a
                // best-effort aid, not a correctness-critical input.
                read_working_tree_file(&path)
                    .ok()
                    .map(|content| (path, content))
            })
            .collect(),
    };
    Ok(Some(TagsResolver::new(
        files,
        language_for_path,
        &reference_names,
        // Same CLI→core polarity flip as the `analyze_diff` /
        // `analyze_repo` calls above (ADR 0025).
        !cli.exclude_tests,
        &generated_paths,
        cli.include_generated,
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
fn list_repo_files_for_outline(cwd: Option<&std::path::Path>) -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    list_git_files(cwd).context(
        "run rinkaku inside a git repository, or pipe a diff (e.g. `gh pr diff 123 | rinkaku`) \
         or pass --base <ref>",
    )
}

/// Resolves which of `paths` are marked "not worth diffing" in
/// `.gitattributes` (ADR 0010): the `diff` attribute is unset (`-diff`,
/// git renders it as binary) or `linguist-generated` is set. Runs
/// `git check-attr -z diff linguist-generated -- <paths...>` in `cwd` (or
/// the process's current directory when `None`) and parses its output with
/// the pure [`parse_generated_paths`].
///
/// Returns an empty set (rather than an error) whenever `git check-attr`
/// itself cannot run or fails — e.g. `paths` is empty (nothing to check,
/// `git check-attr` would otherwise still run happily but there is no
/// point), or `cwd` is not inside a git repository at all. ADR 0010 treats
/// attribute filtering as best-effort: a repository with no
/// `.gitattributes`, or input that isn't backed by a local repository at
/// all (see `resolve_generated_paths_for_stdin`), must not turn into a hard
/// error for the primary diff-condensation flow.
fn check_generated_paths(
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

/// `check_generated_paths`'s sibling for [`build_resolver`]'s repo-wide
/// index: resolves the same `.gitattributes` generated-file set (ADR
/// 0010), but for `paths` drawn from `git ls-files`'s full tracked-file
/// list rather than a diff's changed files — potentially thousands of
/// paths, large enough that passing them as CLI arguments the way
/// `check_generated_paths` does risks exceeding the OS's `ARG_MAX`. Streams
/// them to `git check-attr --stdin -z diff linguist-generated` over stdin
/// instead, which accepts the exact same NUL-separated encoding on input
/// that `-z` alone produces on output (verified against a real
/// `git check-attr --stdin -z` run), so [`parse_generated_paths`] parses
/// this call's stdout unchanged.
///
/// Same best-effort contract as `check_generated_paths`: any failure (not
/// a git repository, `git` not runnable, a non-UTF-8 stream) degrades to
/// an empty set rather than propagating an error, since this powers an aid
/// (the dependency index) rather than the primary diff-condensation flow.
///
/// Writes to stdin on a dedicated thread while reading stdout on the main
/// thread, rather than writing everything up front and reading afterward —
/// same deadlock-avoidance rationale as `read_git_show_files_batch`'s doc
/// comment: with thousands of paths, `git check-attr`'s stdout (an OS pipe
/// with a bounded buffer) can fill up while this process is still writing
/// stdin, and neither side would ever unblock the other without a
/// concurrent reader/writer split.
fn check_generated_paths_batch(
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

/// Parses `git check-attr -z diff linguist-generated -- <paths...>`'s
/// NUL-separated stdout into the set of paths ADR 0010 considers
/// "generated": the `diff` attribute is `unset` (a `.gitattributes` line
/// with `-diff`) or `linguist-generated` is set to a truthy value.
/// `git check-attr` reports two different value strings for
/// `linguist-generated` depending on how `.gitattributes` spells the
/// assignment (verified against a real `git check-attr -z` run): a bare
/// `linguist-generated` (no `=...`) reports the boolean attribute value
/// `set`, while `linguist-generated=true` — GitHub's own Linguist
/// convention and the common real-world spelling — reports the literal
/// string `true` instead. Both are treated as generated; any other value
/// (`unspecified`, `unset`, or an explicit `linguist-generated=false`) is
/// not.
///
/// Output shape (`git help check-attr`'s `-z` mode): a flat stream of
/// `<path>\0<attribute>\0<value>\0` triples, one triple per
/// `(path, attribute)` pair queried — so for two paths and two attributes,
/// four triples in path-major order. Split out from `check_generated_paths`
/// so the parsing logic is unit-testable without shelling out to `git`
/// (CLAUDE.md's boundary-testing policy).
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

/// Reads the diff from stdin. Errors with a clear message if stdin is a
/// terminal (interactive), since there is nothing to read in that case and
/// `--base` should be used instead.
///
/// In practice, `main`'s `if`/`else if` chain already routes every
/// stdin-is-a-TTY, no-`--base`/`--pr` invocation to ADR 0017's whole-repo
/// outline before this function is ever called, so this bail is currently
/// unreachable from that chain. Kept anyway as a defensive check against
/// future callers of this function or a restructured chain in `main`.
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
/// 2. `fetch_base_branch` (`git fetch origin <base_ref_name>`, returning
///    the fetched tip's SHA) then re-check `object_exists` — an ordinary
///    branch fetch usually retrieves it, since `base_ref_oid` is normally
///    reachable from the base branch's history. A failure here (e.g. the
///    base branch was deleted after the PR merged, or renamed) is soft:
///    `log::warn!` and fall through to step 3 rather than aborting the
///    whole run — step 3 is exactly the recovery path for a base branch
///    that no longer leads to `base_ref_oid`, so a step-2 failure must not
///    short-circuit past it.
/// 3. `fetch_oid` (`git fetch origin <oid>`) then re-check `object_exists`
///    — covers a base branch that has since been force-pushed past it,
///    renamed, or deleted (including the case where step 2 itself failed
///    to fetch at all).
/// 4. Fall back to the base branch's tip with `used_fallback` signaling
///    the caller should warn — the commit is unreachable by any means
///    available, so this degrades rather than fails the whole run. Reuses
///    step 2's fetched tip when step 2 succeeded, rather than fetching the
///    same branch a second time; only calls `fetch_base_branch` again here
///    if step 2 itself failed (so there is no tip yet to reuse).
///
/// Every IO step is injected as a closure so this decision logic is
/// unit-testable without shelling out to `git`, following the same
/// pattern as `select_matching_clone` elsewhere in this file.
///
/// Returns the resolved SHA and whether the fallback (step 4) was used.
fn resolve_pr_base_sha(
    base_ref_oid: &str,
    mut object_exists: impl FnMut(&str) -> bool,
    mut fetch_base_branch: impl FnMut() -> anyhow::Result<String>,
    mut fetch_oid: impl FnMut(&str) -> anyhow::Result<()>,
) -> anyhow::Result<(String, bool)> {
    if object_exists(base_ref_oid) {
        return Ok((base_ref_oid.to_string(), false));
    }

    let branch_tip = match fetch_base_branch() {
        Ok(tip) => {
            if object_exists(base_ref_oid) {
                return Ok((base_ref_oid.to_string(), false));
            }
            Some(tip)
        }
        Err(source) => {
            log::warn!(
                "fetching the base branch failed, continuing the base-commit resolution \
                 cascade: {source}"
            );
            None
        }
    };

    if fetch_oid(base_ref_oid).is_ok() && object_exists(base_ref_oid) {
        return Ok((base_ref_oid.to_string(), false));
    }

    let branch_tip = match branch_tip {
        Some(tip) => tip,
        None => fetch_base_branch()?,
    };
    Ok((branch_tip, true))
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

/// Resolves the repository root the TUI's source drill-down
/// (`rinkaku_tui::run`'s `repo_root` parameter) should read files under —
/// `Report` paths are always repository-root-relative (produced by `git
/// diff`/`git ls-files` output, never by the process's own current
/// directory), so `rinkaku-tui/src/source.rs`'s file reads need this to
/// join against rather than the process's current directory directly,
/// which only happens to be the repository root when `rinkaku` is invoked
/// from there.
///
/// Runs `git rev-parse --show-toplevel` in `cwd` (or the process's current
/// directory when `None`) and returns its output — `cwd` here must be the
/// exact same directory the `Report` passed to `rinkaku_tui::run` was
/// built against, i.e. `main`'s own `resolved_workdir` (mirroring
/// `--pr`/`--base`'s `git diff`/`git show` calls, which already run
/// scoped to that same `cwd`/`workdir`). Passing `None` (meaning "the
/// process's current directory") when the `Report` actually came from a
/// `--pr`-resolved ghq/cache clone elsewhere on disk would silently
/// resolve the *process's* repository root instead — the source view
/// would then read whatever unrelated file happens to sit at the same
/// relative path there, rather than erroring, if the process's cwd is
/// itself some other git repository. Falls back to `cwd` unchanged (or the
/// process's actual current directory, via `std::env::current_dir`, when
/// `cwd` is `None`) when the command fails — stdin-diff mode reaches the
/// TUI without ever requiring a git repository at all (`main`'s stdin arm
/// calls `read_working_tree_file` directly, with no `list_git_files`-style
/// gate), so this must degrade gracefully rather than erroring out for a
/// use case ADR 0016/0017 already treat as legitimate.
fn resolve_repo_root(cwd: Option<&std::path::Path>) -> std::path::PathBuf {
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
fn read_git_show_files_batch(
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
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use std::collections::HashSet;

    #[test]
    fn should_default_to_markdown_head_and_no_base_when_no_args_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_tui_when_tui_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: true,
        };
        let actual = Cli::parse_from(["rinkaku", "--tui"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_tui_and_format_given_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--tui", "--format", "json"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_reject_format_and_tui_given_together_regardless_of_argument_order() {
        // clap's conflicts_with is declared on `format` (see Cli's own
        // `#[arg(...)]` attribute), but conflicts are symmetric regardless
        // of which flag declares the attribute or which one is passed
        // first on the command line — this pins that symmetry rather than
        // only ever exercising the --tui-first ordering above.
        let actual = Cli::try_parse_from(["rinkaku", "--format", "json", "--tui"]);

        assert!(actual.is_err());
    }

    #[rstest]
    #[case::should_choose_tui_when_tui_flag_is_set_and_stdout_is_a_terminal(
        true,
        None,
        true,
        DisplayMode::Tui
    )]
    #[case::should_choose_tui_when_tui_flag_is_set_and_stdout_is_not_a_terminal(
        true,
        None,
        false,
        DisplayMode::Tui
    )]
    #[case::should_choose_explicit_format_over_terminal_stdout(
        false,
        Some(Format::Json),
        true,
        DisplayMode::Output(Format::Json)
    )]
    #[case::should_choose_explicit_format_over_non_terminal_stdout(
        false,
        Some(Format::Md),
        false,
        DisplayMode::Output(Format::Md)
    )]
    #[case::should_default_to_tui_when_neither_flag_is_set_and_stdout_is_a_terminal(
        false,
        None,
        true,
        DisplayMode::Tui
    )]
    #[case::should_default_to_markdown_when_neither_flag_is_set_and_stdout_is_not_a_terminal(
        false,
        None,
        false,
        DisplayMode::Output(Format::Md)
    )]
    fn resolve_display_mode_cases(
        #[case] tui: bool,
        #[case] format: Option<Format>,
        #[case] stdout_is_tty: bool,
        #[case] expected: DisplayMode,
    ) {
        let actual = resolve_display_mode(tui, format, stdout_is_tty);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_base_when_base_flag_given() {
        let expected = Cli {
            command: None,
            base: Some("main".to_string()),
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
            format: Some(Format::Json),
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
            format: None,
            deps: 0,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
    fn should_set_exclude_tests_when_exclude_tests_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: true,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--exclude-tests"]);

        assert_eq!(expected, actual);
    }

    // ADR 0025's flipped default: with no test-related flag given, the
    // parsed `Cli` must land on `exclude_tests: false` — i.e. tests are
    // included in Change graph/Definitions by default. The
    // `should_default_to_markdown_head_and_no_base_when_no_args_given`
    // above already exercises the whole default `Cli` shape, but this
    // one pins the ADR 0025 decision specifically so a future default
    // flip has to update this test on purpose rather than by
    // consequence.
    #[test]
    fn should_default_to_including_tests_when_no_flag_given() {
        let actual = Cli::parse_from(["rinkaku"]);

        assert_eq!(false, actual.exclude_tests);
    }

    // Companion to the above: passing the old `--include-tests` flag
    // must now fail parsing, so a stale script surfaces as an error
    // instead of silently doing nothing. Pins the CLI break called out
    // in ADR 0025's Consequences.
    #[test]
    fn should_reject_the_removed_include_tests_flag() {
        let actual = Cli::try_parse_from(["rinkaku", "--include-tests"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_include_generated_when_include_generated_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: true,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--include-generated"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_entry_when_entry_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: Some("src/api".to_string()),
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--entry", "src/api"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_self_update_command_when_self_update_subcommand_given() {
        let expected = Cli {
            command: Some(Command::SelfUpdate { yes: false }),
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
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
                    Ok("branch-tip-sha".to_string())
                },
                |_oid| {
                    *fetch_oid_calls.borrow_mut() += 1;
                    Ok(())
                },
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
                || Ok("branch-tip-sha".to_string()),
                |_oid| panic!("fetch_oid must not be called when the base branch fetch sufficed"),
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
                || Ok("branch-tip-sha".to_string()),
                |_oid| Ok(()),
            )
            .expect("should resolve without error");

            assert_eq!(("base789".to_string(), false), actual);
        }

        #[test]
        fn should_fall_back_to_branch_tip_when_the_oid_is_unreachable_by_any_means() {
            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || Ok("branch-tip-sha".to_string()),
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
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
                || Ok("branch-tip-sha".to_string()),
                |_oid| Ok(()),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
        }

        // Regression test for the must-fix correctness bug: a step-2
        // fetch failure (e.g. the base branch was deleted or renamed after
        // the PR merged) must not abort the whole cascade — step 3 (fetch
        // the oid directly) is exactly the recovery path for this
        // situation, so it must still run and can still resolve
        // `base_ref_oid` even though step 2 failed.
        #[test]
        fn should_fall_through_to_fetch_oid_when_fetching_the_base_branch_fails() {
            let exists_calls = RefCell::new(0);
            let object_exists = |_oid: &str| {
                let mut calls = exists_calls.borrow_mut();
                *calls += 1;
                // Only the initial check happens before the failed branch
                // fetch (which does not re-check); the check after
                // `fetch_oid` (second call) succeeds.
                *calls > 1
            };

            let actual = resolve_pr_base_sha(
                "base789",
                object_exists,
                || anyhow::bail!("simulated: base branch was deleted"),
                |_oid| Ok(()),
            )
            .expect("a step-2 failure must not abort the cascade");

            assert_eq!(("base789".to_string(), false), actual);
        }

        // Sibling case: if step 3 also can't resolve the oid after a
        // step-2 failure, the cascade must still fall back (step 4) rather
        // than propagating the step-2 error — step 2's failure was already
        // handled by falling through, not by failing the whole call.
        #[test]
        fn should_fetch_branch_tip_for_fallback_when_step_two_failed_and_fetch_oid_also_fails() {
            let fetch_base_branch_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || {
                    let mut calls = fetch_base_branch_calls.borrow_mut();
                    *calls += 1;
                    if *calls == 1 {
                        anyhow::bail!("simulated: base branch was deleted")
                    } else {
                        // Step 4 must re-fetch since step 2 never produced
                        // a tip to reuse.
                        Ok("branch-tip-sha".to_string())
                    }
                },
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
            assert_eq!(2, *fetch_base_branch_calls.borrow());
        }

        // Regression test for the must-fix cleanup: when step 2 succeeded
        // (returned a tip) but didn't make `base_ref_oid` resolvable, and
        // step 3 also fails, step 4's fallback must reuse step 2's tip
        // rather than fetching the same base branch a second time.
        #[test]
        fn should_reuse_step_two_tip_for_fallback_without_refetching() {
            let fetch_base_branch_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || {
                    *fetch_base_branch_calls.borrow_mut() += 1;
                    Ok("branch-tip-sha".to_string())
                },
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
            assert_eq!(
                1,
                *fetch_base_branch_calls.borrow(),
                "fetch_base_branch must only be called once (by step 2); step 4 must reuse its \
                 result instead of fetching the base branch again"
            );
        }

        #[test]
        fn should_propagate_error_when_the_branch_tip_fallback_itself_fails() {
            let fetch_base_branch_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || {
                    let mut calls = fetch_base_branch_calls.borrow_mut();
                    *calls += 1;
                    anyhow::bail!("simulated: git fetch origin main failed")
                },
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
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

    // resolve_generated_paths takes already-parsed changed paths (a
    // Vec<String>) rather than the raw diff text, so it cannot re-parse the
    // diff itself — parsing happens exactly once at the call site and the
    // resulting paths are shared with analyze_diff's own parse (still
    // unavoidable, since analyze_diff needs the full ChangedFile data, not
    // just paths).
    #[test]
    fn should_resolve_generated_paths_from_already_parsed_changed_paths() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        std::fs::write(dir.path().join(".gitattributes"), "Cargo.lock -diff\n")
            .expect("write .gitattributes");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let changed_paths = vec!["Cargo.lock".to_string()];
        let actual = resolve_generated_paths(&cli, &changed_paths, Some(dir.path()));

        let expected: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_set_when_include_generated_is_true() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        std::fs::write(dir.path().join(".gitattributes"), "Cargo.lock -diff\n")
            .expect("write .gitattributes");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: true,
            entry: None,
            tui: false,
        };
        let changed_paths = vec!["Cargo.lock".to_string()];
        let actual = resolve_generated_paths(&cli, &changed_paths, Some(dir.path()));

        let expected: HashSet<String> = HashSet::new();
        assert_eq!(expected, actual);
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

    // Regression test for the review blocker: `main`'s `DisplayMode::Tui`
    // arm used to call `resolve_repo_root(None)` unconditionally, ignoring
    // `--pr`'s resolved `workdir` (a ghq/cache clone that can be anywhere
    // on disk, `resolve_pr_workdir`). If the process's own current
    // directory happened to be a *different* git repository, the TUI's
    // source view would silently resolve *that* repository's root and read
    // whatever unrelated file sits at the same relative path there,
    // instead of erroring or reading the PR's actual clone.
    //
    // This proves the mechanism `main` must use (pass the resolved
    // `workdir`/`cwd`, not `None`, whenever the `Report` was built from
    // one) actually reaches the other repository rather than falling back
    // to `pr_repo`: two independent repositories are set up, each with its
    // own `src/lib.rs` at the same relative path, and `resolve_repo_root`
    // is called with `pr_repo`'s path while the *process's* current
    // directory is left at `process_repo` — `resolve_repo_root(None)`
    // (the pre-fix call) would resolve `process_repo`, while
    // `resolve_repo_root(Some(pr_repo))` (the fix) must resolve `pr_repo`.
    #[test]
    fn should_resolve_pr_workdir_root_not_process_cwd_repo_when_both_are_git_repos() {
        let process_repo = tempfile::TempDir::new().expect("create process repo tempdir");
        init_repo_with_committed_file(process_repo.path(), "fn process_repo_marker() {}\n");
        let pr_repo = tempfile::TempDir::new().expect("create pr repo tempdir");
        init_repo_with_committed_file(pr_repo.path(), "fn pr_repo_marker() {}\n");

        // `None` (the pre-fix call the blocker flagged) would have resolved
        // wherever the test process's actual cwd happens to sit — not
        // asserted here since that is the whole point of the bug, only
        // exercised for contrast against the fixed call below.
        let actual = resolve_repo_root(Some(pr_repo.path()));

        let expected = pr_repo
            .path()
            .canonicalize()
            .expect("canonicalize expected");
        let actual = actual.canonicalize().expect("canonicalize actual");
        assert_eq!(expected, actual);
        assert_ne!(
            process_repo
                .path()
                .canonicalize()
                .expect("canonicalize process_repo"),
            actual,
            "must not resolve the unrelated process-cwd repository"
        );
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
            format: None,
            deps: 0,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        // Never called if `deps == 0` truly short-circuits before doing
        // any work at all — deliberately panics so a regression that
        // starts calling it would fail loudly rather than silently
        // reading an empty string.
        let read_file = |_: &str| -> std::io::Result<String> {
            panic!("read_file must not be called when deps == 0")
        };

        let spinner = Spinner::start("test");
        let actual = build_resolver(&cli, "", read_file, None, Some(dir.path()), &spinner)
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
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let read_file = |_: &str| -> std::io::Result<String> { Ok(String::new()) };

        let spinner = Spinner::start("test");
        let actual = build_resolver(&cli, "", read_file, None, Some(dir.path()), &spinner);

        assert!(actual.is_err());
    }

    // Regression test for the must-fix performance/correctness bug: an
    // empty diff (base == head, e.g. `--pr` on an already-merged PR before
    // ADR 0007's fix, or `--base main --head main`) must return the empty
    // `Report` directly, without ever invoking `build_resolver`'s
    // repository-wide `git ls-files` scan. Unlike `deps == 0`'s sibling
    // tests above (which call `build_resolver` directly and can simply
    // point `cwd` at a non-git directory), `run_base_pipeline` calls
    // `run_git_diff` unconditionally first — a non-git `cwd` would make
    // that fail too, before the empty-diff branch is ever reached. So this
    // test instead uses a real repository (required for `run_git_diff` to
    // succeed) and revokes read permission on `.git/index` specifically:
    // `git diff <base>...<head>` (a tree-to-tree comparison between two
    // commits) never opens the index, but `git ls-files` always does — so
    // if `build_resolver` were reached, `list_git_files` would fail and
    // this test would observe `Err` instead of the expected `Ok`.
    #[test]
    fn should_skip_repository_scan_when_diff_is_empty() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo() {}\n");
        let index_path = dir.path().join(".git/index");
        let mut permissions = std::fs::metadata(&index_path)
            .expect("read .git/index metadata")
            .permissions();
        let original_mode = std::os::unix::fs::PermissionsExt::mode(&permissions);
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o000);
        std::fs::set_permissions(&index_path, permissions).expect("revoke .git/index read access");

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let actual = run_base_pipeline(&cli, "HEAD", "HEAD", Some(dir.path()), &spinner);

        // Restore permissions before asserting so a failed assertion
        // doesn't leave an unreadable file behind for the tempdir cleanup.
        let mut permissions = std::fs::metadata(&index_path)
            .expect("re-read .git/index metadata")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, original_mode);
        std::fs::set_permissions(&index_path, permissions).expect("restore .git/index permissions");

        let (actual_report, _actual_diff_text) =
            actual.expect("empty diff must not touch the repository-wide index scan");
        assert_eq!(
            rinkaku_core::render::Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: Vec::new(),
                skipped: Vec::new(),
                graph: rinkaku_core::graph::SymbolGraph {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    roots: Vec::new(),
                },
                tests: Vec::new(),
                hotspots: Vec::new(),
                file_size_warnings: Vec::new(),
                removed: Vec::new(),
            },
            actual_report
        );
    }

    // Regression test (companion to garbage_input_note_tests below): a real
    // `--base` run whose diff touches only a test function must produce a
    // Report with a non-empty `tests` summary and empty `files`/`skipped` —
    // the exact shape garbage_input_note must treat as "a legitimate
    // result", not "garbage input". This exercises the run_base_pipeline
    // route end to end (a real git repo, real analyze_diff call), while the
    // garbage_input_note_tests module below exercises the function in
    // isolation with the same report shape; together they cover both the
    // stdin and --base/--pr code paths that call garbage_input_note (both
    // funnel through analyze_diff, so run_base_pipeline's coverage extends
    // to the stdin route too).
    #[test]
    fn should_produce_test_only_report_without_garbage_input_shape_when_diff_touches_only_a_test_under_exclude_tests()
     {
        // ADR 0025 flipped the default to include tests, so the
        // "test-only diff produces empty files + non-empty tests
        // summary" shape this test pins down only occurs under
        // `--exclude-tests`. The regression this guards is still real:
        // garbage_input_note must not flag such a legitimate result as
        // garbage input, and the test-detection wiring must actually
        // populate `Report.tests` when the flag is set.
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(
            dir.path(),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(1, 1 + 0);
}
",
        );
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(2, 1 + 1);
}
",
        )
        .expect("edit src/lib.rs");
        run_git(dir.path(), &["add", "src/lib.rs"]);
        run_git(dir.path(), &["commit", "-m", "fix test assertion"]);

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            exclude_tests: true,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let (actual, _diff_text) =
            run_base_pipeline(&cli, "HEAD~1", "HEAD", Some(dir.path()), &spinner)
                .expect("run_base_pipeline should succeed for a test-only diff");

        let expected_files: Vec<rinkaku_core::render::FileReport> = Vec::new();
        let expected_skipped: Vec<rinkaku_core::render::SkippedFile> = Vec::new();
        assert_eq!(expected_files, actual.files);
        assert_eq!(expected_skipped, actual.skipped);
        assert_eq!(1, actual.tests.len());
        // The Report shape actually produced is the exact input
        // garbage_input_note must not flag — pin that contract down
        // directly here rather than only trusting the isolated unit test.
        assert_eq!(
            None,
            garbage_input_note("dummy non-empty diff text", &actual)
        );
    }

    // Companion to the above under the new default: a test-only diff
    // with `exclude_tests: false` (the ADR 0025 default) should now put
    // the test symbol into `files` like any production symbol, and
    // leave `tests` empty. Pins that the flag actually flips the
    // resulting shape — without this, the previous test would only
    // prove the exclusion branch, and a regression that ignored the
    // flag entirely could pass.
    #[test]
    fn should_include_test_symbol_in_files_when_diff_touches_only_a_test_under_default() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(
            dir.path(),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(1, 1 + 0);
}
",
        );
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(2, 1 + 1);
}
",
        )
        .expect("edit src/lib.rs");
        run_git(dir.path(), &["add", "src/lib.rs"]);
        run_git(dir.path(), &["commit", "-m", "fix test assertion"]);

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let (actual, _diff_text) =
            run_base_pipeline(&cli, "HEAD~1", "HEAD", Some(dir.path()), &spinner)
                .expect("run_base_pipeline should succeed for a test-only diff");

        let expected_tests: Vec<rinkaku_core::render::TestFileSummary> = Vec::new();
        assert_eq!(expected_tests, actual.tests);
        assert_eq!(1, actual.files.len());
        assert_eq!(1, actual.files[0].symbols.len());
        assert_eq!(true, actual.files[0].symbols[0].is_test);
    }

    // ADR 0014 end-to-end: `run_base_pipeline` must actually wire a
    // `read_base_file` port backed by `git show <base>:<path>` into
    // `analyze_diff`, so a real `--base`/`--pr` run classifies a
    // signature-changing edit as `signature_changed` — not just that the
    // pure `classify_symbols`/`analyze_diff` functions can do so when fed a
    // base reader directly (already covered by
    // `extract::tests::classification_tests` and
    // `pipeline::tests::classification_wiring_tests`).
    #[test]
    fn should_classify_symbol_as_signature_changed_via_real_base_commit() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo(a: i32) -> i32 {\n    a\n}\n");
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn foo(a: i32, b: i32) -> i32 {\n    a\n}\n",
        )
        .expect("edit src/lib.rs");
        run_git(dir.path(), &["add", "src/lib.rs"]);
        run_git(dir.path(), &["commit", "-m", "widen foo's signature"]);

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let (actual, _diff_text) =
            run_base_pipeline(&cli, "HEAD~1", "HEAD", Some(dir.path()), &spinner)
                .expect("run_base_pipeline should succeed");

        let symbol = &actual.files[0].symbols[0];
        assert_eq!(
            Some(rinkaku_core::extract::Classification::SignatureChanged),
            symbol.classification
        );
        assert_eq!(
            Some("fn foo(a: i32) -> i32".to_string()),
            symbol.previous_signature
        );
    }

    mod garbage_input_note_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_core::render::Report;

        fn empty_graph() -> rinkaku_core::graph::SymbolGraph {
            rinkaku_core::graph::SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            }
        }

        fn empty_report() -> Report {
            Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            }
        }

        fn non_empty_report() -> Report {
            Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![rinkaku_core::render::FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![],
                }],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
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
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![rinkaku_core::render::SkippedFile {
                    path: "assets/logo.png".to_string(),
                    reason: rinkaku_core::render::SkipReason::Binary,
                }],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };

            let actual = garbage_input_note("some diff text", &report);

            assert_eq!(None, actual);
        }

        // Regression test: a diff that touches only test symbols produces a
        // Report with empty files/skipped but a non-empty tests summary
        // (ADR 0009's default exclusion) — a legitimate, fully-recognized
        // diff, not garbage input. Before this fix, garbage_input_note only
        // checked files/skipped, so it wrongly printed "no file changes
        // recognized" for every test-only diff.
        #[test]
        fn should_return_none_when_report_has_only_test_summary_entries() {
            let report = Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![rinkaku_core::render::TestFileSummary {
                    path: "src/lib.rs".to_string(),
                    symbol_count: 1,
                }],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };

            let actual = garbage_input_note("some diff text", &report);

            assert_eq!(None, actual);
        }

        // Regression test (ADR 0010 follow-up): a diff whose every changed
        // file is `.gitattributes`-generated produces a Report with empty
        // files/tests but a non-empty skipped list of Generated entries —
        // this is still a fully-recognized, legitimate diff, not garbage
        // input, even though the Markdown rendering now hides Generated
        // entries entirely (render.rs's render_markdown) and would
        // therefore render as an empty string. garbage_input_note reads
        // report.skipped directly (never the rendered Markdown string), so
        // this must keep passing without any code change — this test pins
        // that down explicitly rather than leaving it as an implicit
        // consequence of should_return_none_when_report_has_only_skipped_entries
        // above.
        #[test]
        fn should_return_none_when_report_has_only_generated_skip_entries() {
            let report = Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![
                    rinkaku_core::render::SkippedFile {
                        path: "Cargo.lock".to_string(),
                        reason: rinkaku_core::render::SkipReason::Generated,
                    },
                    rinkaku_core::render::SkippedFile {
                        path: "vendor/generated.go".to_string(),
                        reason: rinkaku_core::render::SkipReason::Generated,
                    },
                ],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };

            let actual = garbage_input_note("some diff text", &report);

            assert_eq!(None, actual);
        }
    }

    mod apply_entry_pivot_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_core::diff::LineRange;
        use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
        use rinkaku_core::render::{FileReport, Report};

        fn symbol(name: &str, referenced_names: Vec<&str>) -> ExtractedSymbol {
            ExtractedSymbol {
                id: String::new(),
                name: name.to_string(),
                kind: SymbolKind::Function,
                signature: format!("fn {name}()"),
                range: LineRange { start: 1, end: 1 },
                container: None,
                referenced_names: referenced_names.into_iter().map(str::to_string).collect(),
                dependencies: vec![],
                omitted_dependency_matches: 0,
                is_test: false,
                classification: None,
                previous_signature: None,
            }
        }

        /// `src/api/handler.rs::api` references `src/util.rs::helper` —
        /// pivoting at "src/api" makes "api" the sole root, mirroring
        /// `graph.rs`'s own pivot-root fixtures. `apply_entry_pivot` is a
        /// thin wrapper over `graph::pivot_graph`, so this module's tests
        /// only pin the wrapper's own contract (every other `Report` field
        /// stays untouched, the note is only printed when appropriate), not
        /// pivot root selection itself.
        fn report_with_api_and_util() -> Report {
            let files = vec![
                FileReport {
                    path: "src/api/handler.rs".to_string(),
                    symbols: vec![symbol("api", vec!["helper"])],
                },
                FileReport {
                    path: "src/util.rs".to_string(),
                    symbols: vec![symbol("helper", vec![])],
                },
            ];
            let graph = rinkaku_core::graph::build_graph(&files);
            Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files,
                skipped: vec![],
                graph,
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            }
        }

        #[test]
        fn should_re_root_graph_at_prefix_while_leaving_other_fields_untouched() {
            let report = report_with_api_and_util();

            let actual = apply_entry_pivot(report.clone(), "src/api");

            let expected = Report {
                graph: rinkaku_core::graph::pivot_graph(&report.graph, "src/api"),
                ..report
            };
            assert_eq!(expected, actual);
        }

        #[test]
        fn should_return_no_symbols_under_path_note_when_prefix_matches_nothing() {
            // `entry_pivot_empty_note` now reads `report.graph.roots`
            // directly rather than recomputing `pivot_roots` itself (item 6:
            // avoid pivot-root selection running twice per `--entry`
            // invocation), so its contract requires an already-pivoted
            // report — the same one `apply_entry_pivot` just produced —
            // rather than the raw `build_graph` output `report_with_api_and_util`
            // returns.
            let report = apply_entry_pivot(report_with_api_and_util(), "no/such/path");

            let actual = entry_pivot_empty_note(&report, "no/such/path");

            assert_eq!(
                Some("note: no symbols under no/such/path".to_string()),
                actual
            );
        }

        #[test]
        fn should_return_none_when_prefix_matches_at_least_one_symbol() {
            let report = apply_entry_pivot(report_with_api_and_util(), "src/api");

            let actual = entry_pivot_empty_note(&report, "src/api");

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_report_graph_has_no_nodes_at_all() {
            let report = Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: rinkaku_core::graph::SymbolGraph {
                    nodes: vec![],
                    edges: vec![],
                    roots: vec![],
                },
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            };

            let actual = entry_pivot_empty_note(&report, "src/api");

            assert_eq!(None, actual);
        }
    }

    mod repo_outline_empty_note_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_core::render::Report;

        fn empty_graph() -> rinkaku_core::graph::SymbolGraph {
            rinkaku_core::graph::SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            }
        }

        fn empty_report() -> Report {
            Report {
                origin: rinkaku_core::render::ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                hotspots: vec![],
                file_size_warnings: vec![],
                removed: vec![],
            }
        }

        #[test]
        fn should_return_note_when_report_has_no_files_and_no_removed() {
            let actual = repo_outline_empty_note(&empty_report());

            assert_eq!(
                Some("note: no supported source files found in the repository"),
                actual
            );
        }

        #[test]
        fn should_return_none_when_report_has_file_entries() {
            let report = Report {
                files: vec![rinkaku_core::render::FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![],
                }],
                ..empty_report()
            };

            let actual = repo_outline_empty_note(&report);

            assert_eq!(None, actual);
        }

        // Regression test: `analyze_repo` leaves `removed` empty on every
        // path today (ADR 0017's whole point is that nothing changed, so
        // there is no base side to diff against), but the check still
        // covers it explicitly so a future extension to `analyze_repo`
        // doesn't silently regress this note into firing on a report that
        // does have something to show.
        #[test]
        fn should_return_none_when_report_has_removed_entries() {
            let report = Report {
                removed: vec![rinkaku_core::extract::RemovedSymbol {
                    name: "old_helper".to_string(),
                    kind: rinkaku_core::extract::SymbolKind::Function,
                    path: "src/lib.rs".to_string(),
                    signature: "fn old_helper()".to_string(),
                }],
                ..empty_report()
            };

            let actual = repo_outline_empty_note(&report);

            assert_eq!(None, actual);
        }
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
