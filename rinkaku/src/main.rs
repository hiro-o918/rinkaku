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

mod cli;
mod display;
mod generated_paths;
mod git;
mod github;
mod notes;
mod pipeline;
mod self_update;
mod spinner;

#[cfg(test)]
mod test_util;

use clap::Parser;
use cli::{Cli, Command};
use display::{DisplayMode, resolve_display_mode};
use generated_paths::check_generated_paths_batch;
use git::commands::{list_repo_files_for_outline, resolve_repo_root};
use git::file_read::read_working_tree_file;
use github::base_sha::{
    fetch_branch_head, fetch_oid, fetch_pr_head, object_exists_locally, resolve_pr_base_sha,
};
use github::pr_arg::parse_pr_arg;
use github::pr_info::fetch_pr_info;
use github::workdir::resolve_pr_workdir;
use notes::{
    apply_entry_pivot, entry_pivot_empty_note, garbage_input_note, repo_outline_empty_note,
};
use pipeline::{
    build_resolver, changed_paths, read_stdin_diff, resolve_generated_paths, run_base_pipeline,
};
use rinkaku_core::render::render;
use spinner::{AnalysisPhase, Spinner, phase_message};
use std::io::IsTerminal;

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
        let report = rinkaku_core::pipeline::analyze_diff(
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
