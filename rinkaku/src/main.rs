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
mod progress;
mod self_update;
mod spinner;
mod splash_progress;

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
use progress::AnalysisProgress;
use rinkaku_core::render::{Report, render};
use rinkaku_tui::TuiSession;
use spinner::{AnalysisPhase, Spinner};
use splash_progress::SplashProgress;
use std::io::IsTerminal;
use std::path::PathBuf;

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

    // ADR 0033: the display mode is decided *before* analysis runs, not
    // after a `Report` already exists — `resolve_display_mode` only
    // depends on `cli.tui`/`cli.format`/whether stdout is a terminal, none
    // of which depend on a `Report`, so this ordering was always available
    // and is what lets the `DisplayMode::Tui` branch below enter the
    // alternate screen and start drawing a splash screen before the
    // pipeline's first byte of work runs, instead of only after it
    // finishes.
    let stdout_is_tty = std::io::stdout().is_terminal();
    let display_mode = resolve_display_mode(cli.tui, cli.format, stdout_is_tty);

    match display_mode {
        DisplayMode::Tui => {
            // No stderr spinner in this branch (ADR 0033 decision 1): the
            // splash screen drawn on the alternate screen is this run's
            // only progress feedback, replacing it rather than layering on
            // top of it.
            let mut session = TuiSession::init()?;
            session.draw_splash(&rinkaku_tui::splash::SplashState::label_only(
                spinner::phase_message(AnalysisPhase::Starting),
            ))?;
            let progress = SplashProgress::new(session);
            let outcome = run_analysis(&cli, &progress).map(|analyzed| {
                // `finish_report` is called *before* `into_session_and_notes`
                // below, while `progress` is still the active
                // `AnalysisProgress` — its own `--entry`-empty note (ADR
                // 0019) must go through the same buffering `note` does for
                // every other advisory message in this branch, or it would
                // reintroduce exactly the raw-bytes-mid-redraw bug this ADR
                // amendment fixes, just for one more call site.
                let report = finish_report(&cli, &progress, analyzed.report);
                (report, analyzed.diff_text, analyzed.resolved_workdir)
            });
            let (session, buffered_notes) = progress.into_session_and_notes();
            let (report, diff_text, resolved_workdir) = match outcome {
                Ok(analyzed) => analyzed,
                Err(err) => {
                    // `session` (and with it, `TuiSession`'s `Drop` impl)
                    // is dropped right here, before `err` propagates past
                    // this function — restoring the terminal ahead of
                    // `main`'s `anyhow` error path printing the failure to
                    // stderr, exactly like `rinkaku_tui::run`'s pre-ADR-0033
                    // `EnableMouseCapture`-failure branch already did for
                    // its own early-return case. `buffered_notes` are
                    // flushed here too (before the terminal-restoring drop
                    // completes, but after — `flush_notes` is plain
                    // `eprintln!`, so ordering against the drop itself
                    // doesn't matter for correctness, only that this runs
                    // after the alternate screen is torn down, which
                    // dropping `session` right below guarantees) so a note
                    // buffered before the error (e.g. `used_fallback`'s
                    // warning, ADR 0033) is not silently lost on an
                    // early-return failure.
                    drop(session);
                    flush_notes(buffered_notes);
                    return Err(err);
                }
            };
            let repo_root = resolve_repo_root(resolved_workdir.as_deref());
            let result = session
                .run(&report, &diff_text, cli.entry.as_deref(), &repo_root)
                .map_err(anyhow::Error::from);
            // Flushed after `TuiSession::run` has already restored the
            // terminal (its own postamble, unconditional on both the `Ok`
            // and `Err` paths — see that method's doc comment) — this is
            // the whole point of buffering in the first place: every note
            // accumulated during analysis (empty diff, garbage input, an
            // `--entry` path matching nothing, the PR base-commit
            // fallback) now reaches stderr as clean, ordinary text once
            // the alternate screen is gone, instead of corrupting a splash
            // or entry-screen frame mid-redraw.
            flush_notes(buffered_notes);
            result
        }
        DisplayMode::Output(format) => {
            // Started before any branch below runs and cleared right after
            // the pipeline finishes (`spinner.finish_and_clear()`), so the
            // whole synchronous analysis phase — the only part of a run
            // with no per-symbol feedback of its own — gets a visible
            // heartbeat on stderr. `Spinner::start` is a no-op-looking
            // wrapper around `indicatif`, whose stderr draw target already
            // suppresses drawing when stderr isn't a terminal (see
            // `spinner.rs`'s own doc comment), so this is safe to run
            // unconditionally in every non-TUI input mode, including piped
            // stderr.
            let spinner = Spinner::start(spinner::phase_message(AnalysisPhase::Starting));
            let analyzed = run_analysis(&cli, &spinner)?;
            // Cleared as soon as the `Report` is built, before the
            // `--entry` pivot (pure/instant) and the render call below.
            spinner.finish_and_clear();

            // Unaffected by ADR 0033's note-deferral amendment: `Spinner`
            // leaves `AnalysisProgress::note` at its default (immediate
            // `eprintln!`), since stderr is not being drawn over by
            // anything in this display mode — every note in this branch
            // still reaches stderr the instant it is produced, same as
            // before this ADR existed.
            let report = finish_report(&cli, &spinner, analyzed.report);
            let output = render(&report, format.into())?;
            print!("{output}");
            Ok(())
        }
    }
}

/// Prints every buffered note (ADR 0033's note-deferral amendment) to
/// stderr, in the order [`AnalysisProgress::note`] received them — the
/// flush half of `--tui` mode's buffer-then-flush strategy, called only
/// after the terminal has actually left the alternate screen (both of this
/// function's two call sites in `main` are positioned that way; see each
/// one's own comment).
fn flush_notes(notes: Vec<String>) {
    for note in notes {
        eprintln!("{note}");
    }
}

/// The result of [`run_analysis`]: a built [`Report`], the raw diff text
/// (empty for the whole-repo-outline branch, ADR 0017), and the resolved
/// working directory (`--pr`'s own clone/cache directory, `None` for every
/// other input mode) — grouped into a named struct rather than a tuple so
/// each field keeps its name at the one call site that destructures all
/// three (a positional tuple invites a field-order mix-up the first time
/// this return shape is touched, the same reasoning
/// `rinkaku_tui::DiffPaneSelectionEffects` documents for itself).
struct AnalyzedReport {
    report: Report,
    diff_text: String,
    resolved_workdir: Option<PathBuf>,
}

/// Runs the same `--pr`/`--base`/stdin/whole-repo input-mode chain
/// `main` always has, reporting progress through `progress` (ADR 0033) —
/// `&dyn AnalysisProgress` rather than a concrete `Spinner`/`SplashProgress`
/// so this one function serves both `DisplayMode`s in `main` without
/// duplicating the chain itself. Extracted out of `main` specifically so
/// the `DisplayMode::Tui` branch there can call it with a live
/// `SplashProgress` sitting in between `TuiSession::init` and
/// `TuiSession::run`, while the non-TUI branch calls it with a `Spinner` —
/// the input-mode logic itself does not need to know which one it got.
fn run_analysis(cli: &Cli, progress: &dyn AnalysisProgress) -> anyhow::Result<AnalyzedReport> {
    // Tracks the same `cwd`/`workdir` each branch below already resolves for
    // its own `git`/`gh` calls, so the TUI's source view (`repo_root`,
    // `main`'s own use of this result) reads files from the repository the
    // `Report` was actually built from rather than always the process's
    // current directory — `--pr` in particular can run entirely against a
    // ghq/cache clone elsewhere on disk (`resolve_pr_workdir`), and
    // `resolve_repo_root(None)` would silently resolve the *process's* repo
    // instead, showing an unrelated file if one happens to exist at the
    // same relative path there.
    let mut resolved_workdir: Option<std::path::PathBuf> = None;
    let (report, diff_text) = if let Some(pr_arg) = &cli.pr {
        // Validate the arg and derive the fetch refspec's PR number, but
        // pass the original (trimmed) value — not the parsed number — to
        // `gh pr view` (see that function's doc comment for why).
        let parsed = parse_pr_arg(pr_arg)?;
        let number = parsed.number();
        progress.set_phase(AnalysisPhase::ResolvingPr);
        let workdir = resolve_pr_workdir(&parsed)?;
        resolved_workdir = workdir.clone();
        // ADR 0033: downgraded from `log::info!` to `log::debug!` — this
        // and the sibling milestone lines below duplicate what the
        // spinner/splash's own phase label already shows on every run
        // (ADR 0032 already made the same call for the lines that overlap
        // with `phase_message`'s own text).
        log::debug!("resolving PR #{number} via gh");
        let pr_info = fetch_pr_info(pr_arg.trim())?;
        let cwd = workdir.as_deref();
        log::debug!("fetching PR #{number} head");
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
        log::debug!("resolving PR #{number} base commit");
        let (base_sha, used_fallback) = resolve_pr_base_sha(
            &pr_info.base_ref_oid,
            |oid| object_exists_locally(cwd, oid),
            || fetch_branch_head(&pr_info.base_ref_name, cwd),
            |oid| fetch_oid(cwd, oid),
        )?;
        if used_fallback {
            // ADR 0033: routed through `progress.note` rather than a bare
            // `log::warn!` (which — like every other stderr write this
            // function used to make directly — would otherwise interleave
            // raw bytes into the TUI's alternate-screen frame stream mid-
            // redraw; see `AnalysisProgress::note`'s own doc comment for
            // the dynamic-verification finding that drove this).
            progress.note(format!(
                "warning: could not resolve PR #{number}'s base commit ({base_oid}) locally; \
                 falling back to the current tip of {base_branch}, which may not reproduce the \
                 original PR diff for a merged PR",
                base_oid = pr_info.base_ref_oid,
                base_branch = pr_info.base_ref_name,
            ));
        }
        run_base_pipeline(cli, &base_sha, &head_sha, cwd, progress)?
    } else if let Some(base) = &cli.base {
        run_base_pipeline(cli, base, &cli.head, None, progress)?
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
        log::debug!("no diff input and stdin is a terminal; building a whole-repo outline");
        progress.set_phase(AnalysisPhase::ParsingRepository);
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
        // ADR 0033: reports `(files_done, total)` back through `progress`
        // as `analyze_repo`'s rayon-parallel loop works through `paths` —
        // see `rinkaku_core::progress::OnProgress`'s own doc comment for
        // why this closure must be `Sync` (it is called from worker
        // threads), which `&dyn AnalysisProgress` already satisfies
        // (`AnalysisProgress: Sync`).
        let on_file_progress =
            |done: usize, total: usize| progress.report_file_progress(done, total);
        let report = rinkaku_core::pipeline::analyze_repo(
            &paths,
            read_working_tree_file,
            // Core's `include_tests: bool` keeps its original meaning
            // ("true means include tests"). Only the CLI-side polarity is
            // flipped by ADR 0025, so translate here.
            !cli.exclude_tests,
            &generated_paths,
            cli.include_generated,
            Some(&on_file_progress),
        );
        if let Some(note) = repo_outline_empty_note(&report) {
            progress.note(note.to_string());
        }
        (report, String::new())
    } else {
        let diff_text = read_stdin_diff()?;
        if diff_text.trim().is_empty() {
            progress.note("note: diff is empty, nothing to analyze".to_string());
        }
        let resolver = build_resolver(
            cli,
            &diff_text,
            read_working_tree_file,
            None,
            None,
            progress,
        )?;
        let changed_paths = changed_paths(&diff_text)?;
        let generated_paths = resolve_generated_paths(cli, &changed_paths, None);
        log::debug!("analyzing diff");
        progress.set_phase(AnalysisPhase::AnalyzingDiff);
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
            progress.note(note.to_string());
        }
        (report, diff_text)
    };

    Ok(AnalyzedReport {
        report,
        diff_text,
        resolved_workdir,
    })
}

/// Applies `--entry`'s pivot (ADR 0019) to `report`, reporting the
/// corresponding empty-result note through `progress` (ADR 0033) when
/// applicable — shared by both `DisplayMode` branches in `main`, which used
/// to inline this identically. `progress` rather than a bare `eprintln!`:
/// this function runs inside the `DisplayMode::Tui` branch too, while a
/// `SplashProgress` is still buffering notes rather than printing them
/// immediately (see `AnalysisProgress::note`'s own doc comment).
fn finish_report(cli: &Cli, progress: &dyn AnalysisProgress, report: Report) -> Report {
    if let Some(entry) = &cli.entry {
        let pivoted = apply_entry_pivot(report, entry);
        if let Some(note) = entry_pivot_empty_note(&pivoted, entry) {
            progress.note(note);
        }
        pivoted
    } else {
        report
    }
}
