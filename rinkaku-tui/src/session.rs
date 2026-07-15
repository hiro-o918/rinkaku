//! Terminal lifecycle ownership (ADR 0033), split out of `crate::lib` (ADR
//! 0028's file-size threshold) — [`TuiSession`] and the crate's
//! [`run`] entry point both belong here rather than in `lib.rs` because
//! they are the one piece of this crate that performs IO and holds a live
//! `Terminal` (`lib.rs`'s own module doc comment on the view-model/terminal-
//! adapter split). [`crate::run_app`] (the pure-dispatch event loop
//! `TuiSession::run` calls into) stays in `lib.rs` itself, alongside the
//! `translate_key`/`dispatch_non_source_key`/etc. helpers it composes —
//! only the terminal-lifecycle wrapper around that loop moves here.

use crate::ReviewPorts;
use crate::run_app;
use crate::source::{SourceReader, WorkingTreeSourceReader};
use crate::splash;
use ratatui::crossterm::event;
use ratatui::crossterm::execute;
use rinkaku_core::render::Report;

/// Runs the interactive TUI over `report` until the user quits, taking
/// over the terminal for the duration of the call (raw mode + alternate
/// screen via [`ratatui::try_init`], restored on return **and** on panic —
/// `ratatui::try_init`'s own panic hook covers the latter, so a bug in this
/// crate cannot leave the caller's terminal in raw mode).
///
/// Uses `try_init` rather than [`ratatui::init`] specifically so terminal
/// setup failure (e.g. stdin/stdout is not a TTY at all — piped input,
/// `< /dev/null`, a CI runner) surfaces as an `Err` for `main.rs`'s
/// `anyhow` path to print cleanly and exit 1, instead of `ratatui::init`'s
/// own `.expect(...)` panicking with a raw Rust panic message and exit
/// code 101.
///
/// Also enables mouse capture (`EnableMouseCapture`) so the wheel/trackpad
/// scroll support below can receive `Event::Mouse` at all — without it, a
/// scroll gesture is intercepted by the terminal emulator itself (moving
/// its own scrollback) and never reaches this process. `ratatui::try_init`/
/// `restore` do not touch mouse capture (mouse support is opt-in, unlike
/// raw mode/alternate screen which every TUI needs), so this function
/// enables and disables it itself, and installs its own panic-hook layer
/// (chained *before* calling `try_init`, so `try_init`'s own hook wraps
/// this one and both run on panic — see `try_init`'s own doc comment: "set
/// a panic hook that restores the terminal") to disable mouse capture on
/// panic too, mirroring the guarantee `try_init` already gives raw mode/
/// alternate screen. `EnableMouseCapture` failing outright (as opposed to
/// panicking) is handled the same way: `try_init` has already put the
/// terminal into raw mode/the alternate screen by that point, so this
/// function calls `ratatui::restore()` itself on that path before
/// propagating the error, rather than relying on `?` to skip straight to
/// the caller (which would strand the terminal mid-setup, since that
/// particular failure is a plain `Err`, not a panic the installed hook
/// would catch).
///
/// This is the only function in the crate that touches a real terminal or
/// blocks on input; everything it calls into (`App`, `row_view`, `ui`,
/// `source`) is either pure or an isolated, narrowly-scoped IO call (a
/// single source-file read).
///
/// `diff_text` is the exact same raw diff string every `main.rs` input mode
/// already holds before handing it to `rinkaku_core::pipeline::analyze_diff`
/// — passed through unchanged, not re-fetched or re-derived here, so this
/// crate never runs `git` itself (ADR 0016: `rinkaku-core`/adapters own IO,
/// not `rinkaku-tui`'s view layer beyond the one source-file read
/// `crate::source` already makes).
///
/// `entry_path` is `main.rs`'s `--entry <path>` flag (ADR 0019), passed
/// through unchanged when the user combines it with `--tui`: `None` when
/// `--entry` was not given (the ordinary case), `Some(path)` to open
/// straight into [`crate::app::RightPane::BlastRadius`] (ADR 0023) with the
/// cursor already on the matching tree row (`App::with_entry_pivot`)
/// instead of requiring the reviewer to hunt for the row and press `R`
/// themselves. Note this crate does *not* itself re-root `report.graph` —
/// `main.rs` already applied `--entry`'s `pivot_graph` re-rooting to
/// `report` before calling here (the same `Report` both the TUI and
/// Markdown/JSON render from), so this parameter only drives where the TUI
/// *starts*, not what the underlying graph looks like.
///
/// `repo_root` anchors `Report` paths (always repository-root-relative) for
/// the source drill-down's file reads (`crate::source::load_symbol_source`)
/// — `main.rs` resolves it once at startup (`git rev-parse --show-toplevel`,
/// falling back to the process's current directory outside a git
/// repository) rather than this crate ever shelling out to `git` itself
/// (ADR 0016). Without it, the source view would only work when `rinkaku`
/// happens to be invoked from the repository root.
///
/// Uses [`WorkingTreeSourceReader`] for the source drill-down's file reads
/// (ADR 0047's default) — callers that need `--pr` mode's head-snapshot
/// reader go through [`TuiSession::run`] directly instead, passing their
/// own [`SourceReader`].
///
/// A thin convenience wrapper around [`TuiSession::init`] +
/// [`TuiSession::run`] for callers that have no splash screen to draw
/// in between (ADR 0033) — every terminal-lifecycle detail this doc
/// comment describes lives on `TuiSession` now, see that type's own doc
/// comment for the same guarantees. Passes `None` for
/// [`TuiSession::run`]'s update-check receiver (ADR 0054) — this
/// convenience wrapper has no version-check thread of its own to hand
/// one in from; callers that want the update prompt use `TuiSession::run`
/// directly, as `rinkaku`'s `main.rs` does.
pub fn run(
    report: &Report,
    diff_text: &str,
    entry_path: Option<&str>,
    repo_root: &std::path::Path,
    review_ports: ReviewPorts<'_>,
) -> std::io::Result<()> {
    TuiSession::init()?
        .run(
            report,
            diff_text,
            entry_path,
            repo_root,
            &WorkingTreeSourceReader,
            review_ports,
            None,
        )
        .map(|_update_requested| ())
}

/// Owns the terminal's raw-mode/alternate-screen/mouse-capture lifecycle
/// (ADR 0033), split out of what used to be a single [`run`] call so
/// `main.rs` can draw [`splash::SplashState`] frames on the same terminal
/// while the pre-render analysis pipeline runs synchronously, *before*
/// handing off to the full event loop — without tearing down and
/// re-entering the alternate screen in between, which would flash the
/// terminal and defeat the splash screen's own purpose.
///
/// [`TuiSession::init`] performs exactly the setup [`run`]'s preamble used
/// to (panic-hook chaining, [`ratatui::try_init`], `EnableMouseCapture`,
/// with the same error-path terminal restoration). [`TuiSession::draw_splash`]
/// draws one splash frame on the already-initialized terminal — call it as
/// many times as the pipeline has phase transitions/progress updates to
/// report, all from the same thread that called `init` (ADR 0033 decision
/// 2: no cross-thread terminal access). [`TuiSession::run`] consumes `self`
/// and performs exactly the postamble `run` used to
/// (`DisableMouseCapture` + [`ratatui::restore`]) on both its `Ok` and
/// `Err` paths.
///
/// A [`Drop`] impl calls [`ratatui::restore`] as a safety net for a path
/// that drops a `TuiSession` without ever calling `run` at all (e.g.
/// `main.rs` returning early with a `?` from inside its own analysis
/// branch, after `init` but before a `Report` exists to hand to `run`) —
/// `ratatui::restore` is documented idempotent, so this is safe to run
/// again even after `run`'s own explicit postamble already restored the
/// terminal on the ordinary path.
pub struct TuiSession {
    terminal: ratatui::DefaultTerminal,
}

impl TuiSession {
    /// See the type's own doc comment for the exact setup this performs.
    pub fn init() -> std::io::Result<Self> {
        // Chained *before* `ratatui::try_init`'s own `set_panic_hook` call
        // below, so the hook `try_init` installs wraps this one: on panic,
        // `try_init`'s hook runs `ratatui::restore()` (raw mode/alternate
        // screen) and then this crate's hook (disable mouse capture),
        // rather than mouse capture silently staying enabled in the
        // panicking caller's terminal.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = execute!(std::io::stdout(), event::DisableMouseCapture);
            previous_hook(info);
        }));

        let terminal = ratatui::try_init()?;
        // Restore explicitly on this `?`'s early-return path (rather than
        // letting `?` skip straight past the `ratatui::restore()` call
        // below): `try_init` above already left raw mode/the alternate
        // screen active, and a plain `EnableMouseCapture` IO failure is
        // not a panic, so the panic-hook layer just installed does not
        // run either — without this, that combination would strand the
        // caller's terminal in raw mode/alternate screen with no cleanup
        // at all.
        if let Err(err) = execute!(std::io::stdout(), event::EnableMouseCapture) {
            ratatui::restore();
            return Err(err);
        }
        Ok(Self { terminal })
    }

    /// Draws one splash frame (ADR 0033) on the already-initialized
    /// terminal. Intended to be called repeatedly from `main.rs`'s
    /// analysis call stack as the pipeline moves between phases/reports
    /// file-scan progress — each call is a plain, synchronous
    /// `Terminal::draw`, not a background redraw loop.
    pub fn draw_splash(&mut self, state: &splash::SplashState) -> std::io::Result<()> {
        self.terminal
            .draw(|frame| splash::draw_splash(frame, state))?;
        Ok(())
    }

    /// Runs the interactive TUI's main event loop over `report` until the
    /// user quits, consuming `self` and restoring the terminal
    /// unconditionally before returning — on both the `Ok` and `Err` path,
    /// matching the postamble the pre-ADR-0033 [`run`] function always ran.
    /// See [`run`]'s own doc comment (preserved there) for what `report`,
    /// `diff_text`, `entry_path`, and `repo_root` mean.
    ///
    /// `source_reader` is the source drill-down's file-content port (ADR
    /// 0047): [`WorkingTreeSourceReader`] for every input mode except
    /// `--pr`, for which `main.rs` wires in a `git show`-backed reader so
    /// the source view reflects the PR's head snapshot rather than
    /// whatever happens to be checked out locally.
    ///
    /// `pr_context`/`submitter` (ADR 0048) are both `Some`/`None`
    /// together on [`ReviewPorts`]: `main.rs`'s composition root assembles
    /// a `PrContext` and wires up a `gh`-backed `ReviewSubmitter` only in
    /// `--pr` mode, so sink A (posting a GitHub PR review) is simply
    /// absent from the export menu otherwise, per the ADR's "no implicit
    /// fallback" decision. `ReviewPorts::clipboard` (sink B) is always
    /// required — it never depends on a PR.
    ///
    /// `update_check` (ADR 0054) is the receiving half of the mpsc channel
    /// `main.rs`'s background version-check thread sends a version string
    /// over, `None` when that thread was never spawned
    /// (`RINKAKU_UPDATE_CHECK=0`, or a caller with no such thread — e.g.
    /// [`run`]'s own convenience wrapper). Threaded through unchanged to
    /// [`run_app`]'s event loop, which owns the actual non-blocking
    /// `try_recv` poll.
    ///
    /// Returns whether the reviewer confirmed the update popup before
    /// quitting (`App::update_requested`) alongside the ordinary
    /// `std::io::Result` — `main.rs` uses this to decide whether to run
    /// `self-update` after this call's terminal-restoring postamble below
    /// has already completed, exactly the "update runs after TUI teardown"
    /// ordering ADR 0054 requires.
    // `update_check` pushed this past clippy's 7-argument threshold — see
    // `run_app`'s own `#[allow]` (this method's sole caller) for why a
    // struct wrapper is not worth it here.
    #[allow(clippy::too_many_arguments)]
    pub fn run(
        mut self,
        report: &Report,
        diff_text: &str,
        entry_path: Option<&str>,
        repo_root: &std::path::Path,
        source_reader: &dyn SourceReader,
        review_ports: ReviewPorts<'_>,
        update_check: Option<std::sync::mpsc::Receiver<String>>,
    ) -> std::io::Result<bool> {
        let result = run_app(
            &mut self.terminal,
            report,
            diff_text,
            entry_path,
            repo_root,
            source_reader,
            review_ports,
            update_check,
        );
        let _ = execute!(std::io::stdout(), event::DisableMouseCapture);
        ratatui::restore();
        result
    }
}

impl Drop for TuiSession {
    fn drop(&mut self) {
        // Idempotent per `ratatui::restore`'s own contract: a no-op if
        // `TuiSession::run` already restored the terminal on the ordinary
        // path, a real safety net if `self` is dropped without `run` ever
        // being called (e.g. `main.rs` returning early with `?` from the
        // analysis phase, after `init` succeeded but before a `Report`
        // exists).
        ratatui::restore();
    }
}
