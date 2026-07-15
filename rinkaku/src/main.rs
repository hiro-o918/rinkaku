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

mod browser;
mod cli;
mod clipboard;
mod display;
mod generated_paths;
mod git;
mod github;
mod log_writer;
mod notes;
mod pipeline;
mod progress;
mod self_update;
mod spinner;
mod splash_progress;

#[cfg(test)]
mod test_util;

use browser::SystemBrowserOpener;
use clap::Parser;
use cli::{Cli, Command};
use clipboard::SystemClipboard;
use display::{DisplayMode, resolve_display_mode};
use generated_paths::check_generated_paths_batch;
use git::commands::{list_repo_files_for_outline, resolve_repo_root};
use git::file_read::read_working_tree_file;
use github::base_sha::{
    fetch_branch_head, fetch_oid, fetch_pr_head, object_exists_locally, resolve_pr_base_sha,
};
use github::pr_arg::{PrArg, parse_pr_arg};
use github::pr_info::fetch_pr_info;
use github::remote::{git_remote_origin_url, parse_github_remote};
use github::review::GhReviewSubmitter;
use github::workdir::resolve_pr_workdir;
use log_writer::DeferredLogSink;
use notes::{
    apply_entry_pivot, entry_pivot_empty_note, garbage_input_note, repo_outline_empty_note,
};
use pipeline::{
    build_resolver, changed_paths, read_stdin_diff, resolve_generated_paths, run_base_pipeline,
};
use progress::AnalysisProgress;
use rinkaku_core::render::{Report, render};
use rinkaku_tui::TuiSession;
use rinkaku_tui::locale::detect_locale;
use rinkaku_tui::review::PrContext;
use spinner::{AnalysisPhase, Spinner};
use splash_progress::SplashProgress;
use std::io::IsTerminal;
use std::path::PathBuf;

/// Shared `env_logger` setup for every display mode: `info`-level default
/// (env_logger's own default is error-only, which meant `--pr`/`--base`
/// runs — the ones slow enough to want a heartbeat, see the
/// dependency-index build below — gave no feedback at all while running;
/// `RUST_LOG` still overrides this). Timestamp and module path are
/// dropped: this is a short-lived one-shot CLI, so there is nothing to
/// correlate a timestamp against, and the binary is a single crate, making
/// the module path redundant.
fn logger_builder() -> env_logger::Builder {
    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    builder.format_timestamp(None).format_target(false);
    builder
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(Command::SelfUpdate { yes }) = cli.command {
        // Non-TUI: logs straight to stderr like every other subcommand did
        // before display-mode resolution was moved ahead of logger init.
        logger_builder().init();
        return self_update::run_self_update(yes);
    }

    // ADR 0033: the display mode is decided *before* analysis runs, not
    // after a `Report` already exists — `resolve_display_mode` only
    // depends on `cli.tui`/`cli.format`/whether stdout is a terminal, none
    // of which depend on a `Report`, so this ordering was always available
    // and is what lets the `DisplayMode::Tui` branch below enter the
    // alternate screen and start drawing a splash screen before the
    // pipeline's first byte of work runs, instead of only after it
    // finishes. Determining `display_mode` before `logger_builder().init()`
    // (ADR 0033 amendment) is what lets the `Tui` branch route the logger
    // through a deferring sink from the very first log call, instead of
    // racing the alternate-screen switch against whichever log record
    // fires first.
    let stdout_is_tty = std::io::stdout().is_terminal();
    let display_mode = resolve_display_mode(cli.tui, cli.format, stdout_is_tty);

    match display_mode {
        DisplayMode::Tui => {
            // ADR 0033 amendment: `log::` records bypass `AnalysisProgress`
            // entirely, so they need their own deferral mechanism — a
            // `DeferredLogSink` buffers every record until `release` is
            // called below, once the alternate screen has actually torn
            // down, mirroring `SplashProgress`'s buffered-notes handling
            // for the same underlying bug (raw bytes landing mid-redraw).
            let log_sink = DeferredLogSink::new();
            logger_builder()
                .target(env_logger::Target::Pipe(Box::new(log_sink.clone())))
                .init();
            // Declared before `TuiSession::init` so Rust's drop order (LIFO)
            // runs `session`'s terminal-restoring `Drop` before this guard's
            // log-release `Drop`, on *any* unwind past this point — a panic
            // inside `run_analysis`/`TuiSession::run`, or an early `?`
            // return from `TuiSession::init`/`draw_splash` below, neither of
            // which reaches the explicit `release_log_sink` calls further
            // down. Those explicit calls still run first on the normal exit
            // paths (`release` is idempotent, see `DeferredLogSink::release`),
            // giving deterministic logs-before-notes ordering there; this
            // guard is purely the safety net for the paths that skip them.
            let _log_sink_guard = log_writer::ReleaseGuard::new(log_sink.clone(), std::io::stderr);

            // ADR 0054: a background version check, skippable via
            // `RINKAKU_UPDATE_CHECK=0` (checked here, at the composition
            // root, rather than inside `rinkaku_tui` — env reads are IO,
            // same boundary rule as everything else this module gates).
            // The spawned thread is fire-and-forget: it is never joined,
            // and `self_update::check_update_available`'s own "silent on
            // any failure" contract means a slow or failed network call
            // simply never sends anything, rather than blocking or
            // panicking this thread.
            let update_check = if std::env::var("RINKAKU_UPDATE_CHECK").as_deref() != Ok("0") {
                let (sender, receiver) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    if let Some(version) = self_update::check_update_available() {
                        let _ = sender.send(version);
                    }
                });
                Some(receiver)
            } else {
                None
            };

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
                (
                    report,
                    analyzed.diff_text,
                    analyzed.resolved_workdir,
                    analyzed.pr_head_sha,
                    analyzed.pr_context,
                )
            });
            let (session, buffered_notes) = progress.into_session_and_notes();
            let (report, diff_text, resolved_workdir, pr_head_sha, pr_context) = match outcome {
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
                    release_log_sink(&log_sink);
                    flush_notes(buffered_notes);
                    return Err(err);
                }
            };
            let repo_root = resolve_repo_root(resolved_workdir.as_deref());
            // `--pr` mode never checks the fetched head ref out (this
            // module's own doc comment on the `--pr` read strategy), so the
            // working tree is not a reliable source for the source view's
            // file content there — a `git show <head>:<path>` reader keeps
            // it pinned to the PR's actual head snapshot instead (ADR
            // 0047). Every other input mode keeps reading the working tree,
            // unchanged.
            let pr_source_reader = pr_head_sha.map(|head| git::file_read::PrHeadSourceReader {
                head,
                cwd: resolved_workdir.clone(),
            });
            let source_reader: &dyn rinkaku_tui::source::SourceReader = match &pr_source_reader {
                Some(reader) => reader,
                None => &rinkaku_tui::source::WorkingTreeSourceReader,
            };
            // ADR 0048: sink A (GitHub PR review) is wired up only when
            // `pr_context` resolved; sink B (clipboard) is always
            // available. `GhReviewSubmitter`/`SystemClipboard` are the
            // composition root's only concrete port implementations —
            // `rinkaku-tui` depends on the `ReviewSubmitter`/`ClipboardSink`
            // trait definitions alone.
            let submitter = pr_context.is_some().then_some(&GhReviewSubmitter as _);
            let system_clipboard = SystemClipboard::detect();
            let system_browser = SystemBrowserOpener;
            let review_ports = rinkaku_tui::ReviewPorts {
                pr_context,
                submitter,
                clipboard: &system_clipboard,
                browser: &system_browser,
            };
            // ADR 0055: `?` help overlay locale, detected here (the
            // composition root) from the same POSIX `LC_ALL > LC_MESSAGES
            // > LANG` precedence env reads every other IO in this module
            // is isolated to — `rinkaku_tui::locale::detect_locale` itself
            // is a pure function taking the already-read values.
            let locale = detect_locale(
                std::env::var("LC_ALL").ok().as_deref(),
                std::env::var("LC_MESSAGES").ok().as_deref(),
                std::env::var("LANG").ok().as_deref(),
            );
            let run_result = session.run(
                &report,
                &diff_text,
                cli.entry.as_deref(),
                &repo_root,
                source_reader,
                review_ports,
                update_check,
                locale,
            );
            // Flushed after `TuiSession::run` has already restored the
            // terminal (its own postamble, unconditional on both the `Ok`
            // and `Err` paths — see that method's doc comment) — this is
            // the whole point of buffering in the first place: every note
            // accumulated during analysis (empty diff, garbage input, an
            // `--entry` path matching nothing, the PR base-commit
            // fallback) now reaches stderr as clean, ordinary text once
            // the alternate screen is gone, instead of corrupting a splash
            // or entry-screen frame mid-redraw.
            release_log_sink(&log_sink);
            flush_notes(buffered_notes);
            let update_requested = run_result.map_err(anyhow::Error::from)?;
            // ADR 0054: the update itself runs only after the block above
            // has already restored the terminal — `yes: true` since the
            // reviewer already confirmed inside the TUI's own popup, so
            // `run_self_update` skips straight to downloading rather than
            // prompting a second time on the now-restored terminal.
            if update_requested {
                self_update::run_self_update(true)
            } else {
                Ok(())
            }
        }
        DisplayMode::Output(format) => {
            // Non-TUI: no alternate screen ever opens, so `log::` output
            // goes straight to stderr, same as it always has.
            logger_builder().init();

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

/// Releases a `--tui`-mode [`DeferredLogSink`] to stderr — the `log::`
/// counterpart of [`flush_notes`]. Called explicitly at `main`'s two normal
/// exit points, in the same position as each `flush_notes` call, so logs
/// drain before notes in a fixed order on those paths (`release` is
/// idempotent, so this is safe even though `_log_sink_guard`'s `Drop` will
/// also release the same sink later). Paths that skip these explicit calls
/// (a panic, or an early `?` return before either is reached) still get
/// their buffered records drained by that guard.
fn release_log_sink(sink: &DeferredLogSink<std::io::Stderr>) {
    // A failed `write_all`/`flush` to stderr here has nowhere left to be
    // reported (the process is already on its way out in every call site),
    // so it is dropped rather than propagated — same judgment call
    // `flush_notes`'s own `eprintln!` already makes implicitly.
    let _ = sink.release(std::io::stderr());
}

/// The result of [`run_analysis`]: a built [`Report`], the raw diff text
/// (empty for the whole-repo-outline branch, ADR 0017), the resolved
/// working directory (`--pr`'s own clone/cache directory, `None` for every
/// other input mode), and the resolved PR head SHA (`--pr` only, `None`
/// otherwise) — grouped into a named struct rather than a tuple so each
/// field keeps its name at the one call site that destructures all four (a
/// positional tuple invites a field-order mix-up the first time this
/// return shape is touched, the same reasoning
/// `rinkaku_tui::DiffPaneSelectionEffects` documents for itself).
struct AnalyzedReport {
    report: Report,
    diff_text: String,
    resolved_workdir: Option<PathBuf>,
    pr_head_sha: Option<String>,
    /// The PR's identity, for `--tui`'s review-annotations sink A (ADR 0048) —
    /// `Some` only in `--pr` mode, and only when owner/repo could be
    /// resolved (see [`resolve_pr_context`]'s own doc comment on why that
    /// resolution can fail even in `--pr` mode without failing the whole
    /// run). `None` for every other input mode.
    pr_context: Option<PrContext>,
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
    // Populated only in the `--pr` branch below; carried out to
    // `AnalyzedReport::pr_head_sha` so `main`'s `DisplayMode::Tui` arm can
    // wire a `git show`-backed `SourceReader` (ADR 0047) for exactly this
    // input mode, the same way `resolved_workdir` above already carries
    // out `--pr`'s resolved clone directory.
    let mut pr_head_sha: Option<String> = None;
    let (report, diff_text, pr_context) = if let Some(pr_arg) = &cli.pr {
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
        pr_head_sha = Some(head_sha.clone());
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
        let (report, diff_text) = run_base_pipeline(cli, &base_sha, &head_sha, cwd, progress)?;
        // ADR 0048: `PrContext` for `--tui`'s review-annotations sink A, `None`
        // when owner/repo can't be resolved (`resolve_pr_context`'s own
        // doc comment on why that is a soft failure, not a hard error —
        // the analysis above has already succeeded by this point, and
        // losing sink A is strictly less bad than losing the whole run).
        let pr_context = resolve_pr_context(&parsed, cwd, number, head_sha);
        (report, diff_text, pr_context)
    } else if let Some(base) = &cli.base {
        let (report, diff_text) = run_base_pipeline(cli, base, &cli.head, None, progress)?;
        (report, diff_text, None)
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
        (report, String::new(), None)
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
        // ADR 0033 (amended): same `on_file_progress` shape as the
        // `analyze_repo`/`build_resolver` calls above — reports
        // `(files_done, total)` back through `progress` as `analyze_diff`'s
        // sequential per-file loop works through the diff's changed files.
        let on_file_progress =
            |done: usize, total: usize| progress.report_file_progress(done, total);
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
            Some(&on_file_progress),
        )?;
        if let Some(note) = garbage_input_note(&diff_text, &report) {
            progress.note(note.to_string());
        }
        (report, diff_text, None)
    };

    Ok(AnalyzedReport {
        report,
        diff_text,
        resolved_workdir,
        pr_head_sha,
        pr_context,
    })
}

/// Resolves a [`PrContext`] for `--tui`'s review-annotations sink A (ADR 0048),
/// given the already-validated `parsed` PR arg, the `cwd` `--pr` mode
/// resolved (`resolve_pr_workdir`'s own doc comment), the PR `number`, and
/// its `head_sha` (already fetched and verified against `gh`'s own report
/// by this function's only caller). Owner/repo come straight off `parsed`
/// for [`PrArg::Url`]; for [`PrArg::Number`] (no repository information of
/// its own) they are resolved the same way `--pr` URL mode itself decides
/// which clone to use — `git remote get-url origin` in `cwd`, parsed via
/// [`parse_github_remote`].
///
/// `None` (rather than a hard error) when owner/repo cannot be resolved —
/// a bare `--pr <number>` run inside a clone whose `origin` isn't a GitHub
/// remote at all (or has none) can still analyze and render/render-TUI
/// successfully; only sink A (posting a GitHub PR review) needs this, and
/// losing just that sink is strictly better than failing the whole run
/// over a feature the reviewer may not even reach.
fn resolve_pr_context(
    parsed: &PrArg,
    cwd: Option<&std::path::Path>,
    number: u64,
    head_sha: String,
) -> Option<PrContext> {
    let (owner, repo) = match parsed {
        PrArg::Url { owner, repo, .. } => (owner.clone(), repo.clone()),
        PrArg::Number(_) => {
            let origin = git_remote_origin_url(cwd).ok().flatten()?;
            parse_github_remote(&origin)?
        }
    };
    Some(PrContext {
        owner,
        repo,
        number,
        head_sha,
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
