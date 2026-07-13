# 0033. A TUI splash screen with real progress during pre-render analysis

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0032 added a stderr spinner for the synchronous analysis pipeline that
runs before any output is produced. It covers every input mode, but stops
being useful the moment `--tui` is the selected display mode: `main.rs`
clears the spinner (`Spinner::finish_and_clear`) and only then calls
`rinkaku_tui::run`, which itself does not draw anything until `run_app`'s
event loop reaches its first `terminal.draw` call — after `App::new` and
the up-front `diff_view::parse_diff_hunks` / `highlight::highlight_diff_files`
computation have already run. Between `ratatui::try_init` entering the
alternate screen and that first frame, the terminal shows a blank screen
with no feedback at all — worse than the pre-`--tui` gap ADR 0032 fixed,
because the alternate screen swap itself is a visible flash into
emptiness rather than a spinner line simply continuing to spin.

Two more specific gaps compound this for `--tui`'s default configuration
(`--deps 1`, stdin-is-a-terminal → whole-repo outline, ADR 0017):
`rinkaku_core::pipeline::analyze_repo` and
`rinkaku_core::deps::TagsResolver::new` both scan and parse every tracked
file in the repository, which ADR 0031's profiling table shows can take
seconds on a large tree. The stderr spinner (still shown for these phases
in every *other* display mode) is an indeterminate animation — it proves
the process is alive but gives no sense of how much work remains, which
matters most exactly when the phase is slow.

This ADR is not a revisit of ADR 0031's deferred "progressive TUI
rendering" or "lazy start" alternatives (still deferred): neither
alternative here starts the interactive session against partial data, or
moves the parse to a background thread. The pipeline stays fully
synchronous on the calling thread; what changes is what the terminal
shows while it runs, and, for the two file-scanning phases above, showing
a real (not simulated) fraction-complete count.

## Decision

1. **`rinkaku` decides the display mode before running any analysis.**
   `resolve_display_mode` (introduced by ADR 0032's `main.rs` split) only
   depends on `cli.tui`, `cli.format`, and whether stdout is a terminal —
   none of which depend on a `Report` — so `main` now calls it once,
   immediately after parsing `Cli`, rather than after the pipeline
   produces a `Report`. When the result is `DisplayMode::Tui`, `main`
   enters the alternate screen (`rinkaku_tui::TuiSession::init`) and draws
   the splash screen *before* branching into the `--pr`/`--base`/stdin/
   whole-repo analysis, instead of after. The ADR 0032 stderr spinner is
   skipped entirely in this path (`Spinner::start` is simply not called);
   every other display mode is unaffected and keeps the spinner exactly
   as ADR 0032 left it — the two are mutually exclusive per run, matching
   the two paths already being separate branches of `main`.

2. **No threads or channels.** The splash screen's progress is updated by
   redrawing the same `ratatui::DefaultTerminal` from inside the analysis
   call stack, on the same thread that called `main`. `rinkaku-core`'s
   `analyze_repo` and `deps::TagsResolver::new` — the only two phases with
   real per-file work to report — each gain an `on_progress: Option<&(dyn
   Fn(usize, usize) + Sync)>` parameter (`(files_done, files_total)`),
   defined and consumed entirely inside `rinkaku-core` (a port, per
   CLAUDE.md's "ports as traits/closures, defined on the consumer side"
   rule) so the core stays free of any `ratatui`/terminal dependency.
   `main.rs` supplies a closure that calls into `rinkaku_tui`'s splash
   redraw; every other caller (every existing test, every non-TUI display
   mode) passes `None`, a source-compatible default requiring no changes
   beyond the new parameter itself.

   `analyze_repo`'s per-file loop is already parallelised across rayon
   worker threads (ADR 0031); `on_progress` is called from those worker
   threads too, via an `AtomicUsize` counter each completed file
   increments and a fixed stride (every 16 files) that decides whether
   *this* increment also calls `on_progress` — bounding redraw frequency
   regardless of file count or thread count, rather than calling it on
   every single file (thousands of redraws for a large repository) or
   trying to serialize calls through a single thread (reintroducing the
   cross-thread coordination this ADR explicitly avoids elsewhere).
   `on_progress` itself must therefore be `Sync`; the terminal redraw
   underneath it is not actually reentered concurrently in practice
   despite the `Sync` bound — rayon's worker threads all belong to the
   same process and the redraw itself is a plain synchronous
   `Terminal::draw` call, so the *danger* `Sync` usually protects against
   (two threads mutating the terminal at once) is possible in principle
   but made harmless by the stride: the closure `main.rs` supplies takes a
   `std::sync::Mutex` around the `DefaultTerminal` handle for the
   duration of each redraw specifically to make this safe, rather than
   relying on the low probability of two strided calls landing
   simultaneously.

   `deps::TagsResolver::new`'s indexing loop is sequential (unchanged by
   this ADR — parallelising it is out of scope, see Alternatives), so its
   own `on_progress` call needs no atomic counter: a plain `usize` counter
   incremented once per loop iteration, called every 16 files with the
   same stride constant as `analyze_repo`, on the single calling thread.

3. **The splash view-model is pure, the drawing is not.**
   `rinkaku-tui` gains a new `splash` module: `SplashState { phase_label:
   String, progress: Option<(usize, usize)> }` (plain data, `PartialEq`+
   `Debug`, no `ratatui` types) and a static `LOGO_LINES: &[&str]` ASCII-art
   constant, mirroring `help.rs`'s existing "static content as a `const`,
   not computed" precedent. `draw_splash(frame: &mut Frame, state:
   &SplashState)` is the thin, deliberately-uncovered-beyond-`TestBackend`
   rendering function `ui::draw` already sets the precedent for. The
   progress bar, when `state.progress` is `Some((done, total))`, is a
   determinate `ratatui::widgets::Gauge` filled to `done as f64 / total as
   f64`; when `state.progress` is `None` (every phase except index-building/
   whole-repo-parsing), only `phase_label` is shown under the logo — no
   fake animation stands in for the missing signal, per this task's
   explicit "do not fake progress" requirement.

4. **`rinkaku_tui::run` splits into `TuiSession`.** `pub struct
   TuiSession` owns the live `ratatui::DefaultTerminal` and replaces
   `run`'s previous one-shot preamble/postamble: `TuiSession::init()`
   performs exactly what `run`'s preamble already did (panic-hook
   chaining, `ratatui::try_init`, `EnableMouseCapture`, with the same
   error-path terminal restoration `run`'s own doc comment already
   describes) and returns `io::Result<Self>`; `TuiSession::draw_splash(&mut
   self, state: &splash::SplashState) -> io::Result<()>` draws one splash
   frame on the already-initialized terminal; `TuiSession::run(self,
   report, diff_text, entry_path, repo_root) -> io::Result<()>` consumes
   `self`, runs the existing `run_app` event loop against the terminal it
   already owns, and performs exactly the postamble `run` used to
   (`DisableMouseCapture` + `ratatui::restore()`) — on both the `Ok` and
   `Err` paths, matching `run`'s existing unconditional cleanup. The
   crate's top-level `pub fn run(...)` function is kept, now implemented
   as `TuiSession::init()?.run(...)`, so every existing caller that does
   not need a splash screen (this crate's own doc examples, any future
   embedder) is unaffected.

5. **Error path**: an analysis error occurring after `TuiSession::init`
   (e.g. a failing `git`/`gh` subprocess inside `run_base_pipeline`, same
   call sites ADR 0032 already documents) must not leave the terminal in
   the alternate screen/raw mode. `main.rs`'s `?`-propagation on the
   analysis branch is replaced with explicit handling once a
   `TuiSession` is live: on error, `TuiSession` is dropped/its cleanup
   path is invoked (`ratatui::restore()` + `DisableMouseCapture`) *before*
   the error is formatted to stderr, mirroring `run`'s own existing
   `EnableMouseCapture`-failure branch (that function's doc comment: "this
   function calls `ratatui::restore()` itself on that path before
   propagating the error"). Concretely, `main` wraps the analysis branch
   in a closure/inner function once `TuiSession` is live, and always calls
   `TuiSession`'s explicit teardown method before returning the error —
   there is no `Drop`-based automatic restore on `TuiSession` today (unlike
   `indicatif`'s `BarState`, `ratatui::restore()` is not run implicitly on
   drop), so this ADR adds a `Drop` impl for `TuiSession` that calls
   `ratatui::restore()` (idempotent, matching `ratatui::restore`'s own
   contract) as a safety net for exactly this early-return case, on top of
   `TuiSession::run`'s own explicit postamble on the success path.

6. **Non-TUI parity is preserved by construction**: `on_progress: None` is
   what every non-TUI call site (Markdown/JSON display modes, the
   whole-repo-outline branch reached when stdout is not a terminal, every
   existing test) passes, so `analyze_repo`/`TagsResolver::new`'s
   behavior and output are unchanged whenever a progress callback isn't
   supplied — this is an additive parameter, not a new default.

7. **`log::info!` lines that now duplicate the splash's own label are
   downgraded to `log::debug!`** (e.g. `"analyzing diff"`,
   `"resolving PR #{number} via gh"`) inside the TUI branch's own call
   path, mirroring the same reasoning ADR 0032 already applied when the
   spinner made those lines redundant for non-TUI output.

8. **Advisory notes are buffered during `--tui` mode and flushed only
   after the terminal leaves the alternate screen (amendment, added
   during review).** The original version of this decision left every
   `eprintln!`/`log::warn!` notice (empty diff, garbage input, an
   `--entry` path matching nothing, the PR base-commit fallback) firing
   immediately, on the assumption that "the splash never displays those
   messages, so they must still reach stderr to be seen at all" was
   enough justification to leave them untouched. Dynamic verification
   during review disproved that: `--tui --base <ref> --entry
   <path-matching-nothing>` writes the raw, unstyled note bytes straight
   into the terminal's alternate-screen frame stream — mid-redraw,
   between the splash's "Analyzing diff..." frame and the entry screen's
   first frame — because stderr and the alternate screen are the same
   physical terminal, and nothing about entering the alternate screen
   redirects or suppresses a plain `eprintln!` the way it would for, say,
   `indicatif`'s TTY-aware draw target (ADR 0032's own note on why the
   *spinner* needs no such handling: `indicatif` already detects and
   defers to a non-interactive stream, but a bare `eprintln!` has no such
   awareness at all).

   `AnalysisProgress` (the port `run_base_pipeline`/`build_resolver`/
   `main.rs` already thread through for phase/progress reporting, per
   decision 2 above) gains a third method, `note(&self, message:
   String)`, defaulting to today's immediate `eprintln!` — every non-TUI
   caller (the stderr `Spinner`) leaves it at that default, since stderr
   is not being drawn over by anything outside `--tui` mode.
   `--tui` mode's `SplashProgress` is the one override: it pushes
   `message` onto a `Vec<String>` guarded by the same `Mutex` that
   already serializes its terminal access, instead of printing.
   `main.rs` extracts that buffer via
   `SplashProgress::into_session_and_notes` (renamed from the original
   `into_session`) alongside the plain `TuiSession`, and flushes it with
   a small `flush_notes` helper at exactly two points — after
   `TuiSession::run` returns (both `Ok` and `Err`, since `run`'s own
   postamble unconditionally restores the terminal first) and after an
   early-return analysis error drops the `TuiSession` (whose `Drop`
   safety net, decision 5 above, restores the terminal before the flush
   runs). Every call site that used to call `eprintln!`/`log::warn!`
   directly — `run_base_pipeline`'s two (empty diff, garbage input),
   `main.rs`'s `run_analysis` two (repo-outline-empty, stdin empty-diff),
   `main.rs`'s `finish_report` one (`--entry` empty), and the PR
   base-commit fallback (`log::warn!` → `progress.note`, reclassified as
   a plain note rather than a log line so it is buffered the same way) —
   now goes through `progress.note(...)` instead.

   `finish_report` itself moves *inside* the `--tui` branch's still-live
   `SplashProgress` scope (called with `&progress` before
   `into_session_and_notes` consumes it), rather than after, specifically
   so its own `--entry`-empty note is captured by the same buffer — the
   bug report's exact reproduction used `--entry` for this reason.

## Amendment (review round 1)

Decision 8 above **is** the amendment: it replaces the original
Consequences bullet claiming "`eprintln!` notices ... are unaffected —
the splash never displays those messages, so they must still reach
stderr to be seen at all" with the buffer-then-flush design, after
dynamic verification showed that claim was false — a note written to
stderr during `--tui` mode is not simply "not displayed by the splash",
it is interleaved into the same terminal the splash/entry screen are
actively redrawing, corrupting whichever frame happens to be mid-write
when the note lands.

## Alternatives

- **Thread/channel-based lazy start** (ADR 0031's deferred alternative,
  still deferred): would let the TUI open its main screen immediately and
  fill in rows as parsing completes elsewhere, a strictly larger change
  to `rinkaku-tui`'s state machine and a real concurrency surface (a
  background thread mutating `App`/`Report` state the render loop reads).
  Not what this ADR does — the splash is a *waiting* screen, shown while
  the same synchronous, single-call-stack pipeline ADR 0031/0032 already
  established keeps running, not a mechanism for interacting with partial
  results.
- **Parallelize `TagsResolver::new`'s indexing loop with rayon**, matching
  `analyze_repo`: would give the dependency-index phase the same
  wall-clock win ADR 0031 gave the whole-repo-outline phase, but is a
  larger, independently-reviewable change to `deps.rs`'s sequential
  per-file loop (which — unlike `analyze_repo`'s already-`collect`-shaped
  body — interleaves index insertion with the prefilter's `AhoCorasick`
  match) and is not needed to make its progress observable. Left for a
  future PR/ADR if `TagsResolver::new`'s own wall-clock time (not just its
  visibility) becomes the bottleneck worth solving.
- **A fake/animated progress bar for phases with no real signal**
  (`ResolvingPr`, `Diffing`, `AnalyzingDiff`): rejected outright per this
  feature's own requirement — a bar that doesn't correspond to real work
  either lies about how much is left or, worse, stalls visibly and looks
  broken. A label-only splash for these phases is honest about what is
  and isn't measurable, same spirit as ADR 0032's spinner being
  indeterminate rather than a guessed determinate bar.
- **Route splash progress through `indicatif`** (already a `rinkaku`
  bin-crate dependency, ADR 0032): rejected — `indicatif` draws directly
  to a terminal stream itself, which cannot coexist with `ratatui` already
  owning the alternate screen/raw mode. The splash is drawn with
  `ratatui`, matching CLAUDE.md's "rinkaku-tui does not depend on
  indicatif" requirement and ADR 0032's own "kept in the `rinkaku` bin
  crate, not `rinkaku-tui`" boundary for the stderr spinner specifically
  — the splash inverts that boundary on purpose, since it is TUI-mode-only
  content.
- **Keep `rinkaku_tui::run`'s single-function shape and pass a
  pre-initialized `Option<DefaultTerminal>` in**: considered, but
  `TuiSession` reads more directly as "the TUI's terminal lifecycle,
  explicitly staged" than a function whose behavior branches on whether an
  `Option` argument was pre-filled — the two-phase `init`/`run` split also
  matches the fact that `main.rs` genuinely does two separate things with
  the terminal (draw splash frames during analysis, then hand off to the
  full event loop) rather than one.

## Consequences

- **Core API**: `rinkaku_core::pipeline::analyze_repo` and
  `rinkaku_core::deps::TagsResolver::new` each gain a new
  `on_progress: Option<&(dyn Fn(usize, usize) + Sync)>` parameter (last
  position). Every existing call site (tests included) is updated to pass
  `None`; behavior and output are unchanged when it is `None`. This is a
  breaking signature change for any external caller of these two
  functions (both are `pub`), consistent with this project's
  pre-1.0/`0.x` versioning tolerance for additive-but-signature-breaking
  changes (same class of change ADR 0031 made to `analyze_repo`'s
  `read_file` bound).
- **`rinkaku-tui` API**: adds `pub mod splash` (`SplashState`,
  `LOGO_LINES`, `draw_splash`) and `pub struct TuiSession` with
  `init`/`draw_splash`/`run`. `pub fn run(...)`'s signature is unchanged;
  its body now delegates to `TuiSession`.
- **Dependencies**: no new crate dependencies anywhere — `splash`'s
  progress bar uses `ratatui::widgets::Gauge`, already available via the
  existing `ratatui` dependency; `rinkaku-core`'s new counter uses
  `std::sync::atomic::AtomicUsize`, already in `std`.
- **UX**: `--tui` runs (the default when stdout is a terminal, ADR 0017)
  show the rinkaku logo immediately on startup instead of a blank
  alternate-screen flash, with a real, non-simulated progress bar during
  the two file-scanning phases and a plain phase label otherwise. Every
  non-TUI display mode is byte-for-byte unaffected: `on_progress: None`
  everywhere in that path, and ADR 0032's stderr spinner keeps running
  exactly as before — including its notes, which (decision 8's amendment)
  still print immediately via `AnalysisProgress::note`'s default. `--tui`
  mode's own notes now surface *after* the session ends instead of not at
  all mid-session, which is a UX change from ADR 0032's baseline (a
  reviewer previously saw these notes interleaved with `log::info!` output
  before the TUI even started; now they appear on a clean stderr right
  after quitting/erroring out of the TUI) but strictly better than the
  data-corrupting alternative decision 8 replaces.
- **`rinkaku` bin API** (all `pub(crate)`, no external surface):
  `AnalysisProgress` gains a third method, `note(&self, message: String)`,
  defaulting to `eprintln!`. `SplashProgress::into_session` is renamed to
  `SplashProgress::into_session_and_notes`, returning `(TuiSession,
  Vec<String>)` instead of just `TuiSession`. `finish_report` gains a
  `progress: &dyn AnalysisProgress` parameter (previously just `cli` and
  `report`). A new `flush_notes(Vec<String>)` helper in `main.rs` prints a
  buffer's contents in order.
- **`rinkaku-tui` API**: adds `pub mod splash` (`SplashState`,
  `LOGO_LINES`, `draw_splash`) and `pub struct TuiSession` with
  `init`/`draw_splash`/`run`. `pub fn run(...)`'s signature is unchanged;
  its body now delegates to `TuiSession`.
- **Testing**: `SplashState`/the phase→label mapping/the stride decision
  (`should_report_progress_this_index`, or similarly named) are unit
  tested as pure functions (`rstest` + `pretty_assertions`). `draw_splash`
  itself gets the same coarse `TestBackend` treatment `ui::draw`'s
  submodules already use — a snapshot-style assertion that the logo and
  label/bar appear, not a pixel-exact pin. `AnalysisProgress::note`'s
  contract (an implementer can override it to buffer instead of printing,
  and buffering preserves call order) is unit tested against a
  hand-rolled fake in `progress.rs`, mirroring `SplashProgress`'s actual
  override without needing a real terminal. `TuiSession::init`/`run`'s
  actual terminal lifecycle is not unit-tested, matching ADR 0032's own
  precedent for `Spinner` (no mocking of a real terminal) — covered
  instead by this PR's dynamic verification (pty-driven manual runs).
- **Debuggability**: `TuiSession`'s `Drop`-based `ratatui::restore()`
  safety net means a future early-return added to the TUI analysis branch
  in `main.rs` cannot strand the terminal in raw mode/the alternate
  screen, even without remembering to call the explicit teardown method —
  matching the same "belt and braces" precedent `rinkaku_tui::run`'s own
  panic-hook layering already sets.

## Amendment (2026-07-13)

The "fake/animated progress bar" rejection above (Alternatives) listed
`AnalyzingDiff` alongside `ResolvingPr`/`Diffing` as a phase with "no real
signal" — true at the time, since `analyze_diff`'s per-file loop had no
progress port at all. That has changed: `analyze_diff` now takes the same
`on_progress: Option<rinkaku_core::progress::OnProgress>` port
`analyze_repo` and `TagsResolver::new` already had, reporting
`(files_done, total)` as its sequential loop over the diff's changed files
progresses, gated by the same `should_report_progress` stride rule. `main.rs`
wires this through `AnalysisProgress::report_file_progress` exactly as it
already did for the other two phases, so `AnalyzingDiff` now draws a real
determinate gauge in `--tui` mode instead of a label-only splash — closing
the one phase this ADR originally could not give real progress to.

This does not change any decision above: it exercises the same
`OnProgress`/`should_report_progress` port `analyze_repo` established,
extended to a second, sequential (not rayon-parallel) call site, so no new
concurrency primitive is introduced (a plain `usize` counter suffices,
where `analyze_repo` needs `AtomicUsize` for its parallel workers). "Files
done" counts every changed file the loop looks at, including ones it skips
(deleted/generated/binary/unsupported-language/pure-rename), matching
`analyze_repo`'s own "looked at" — not "produced a report for" — convention,
so a caller watching the callback sees the same meaning regardless of
which pipeline entry point produced it.

- **Core API**: `rinkaku_core::pipeline::analyze_diff` gains the same
  `on_progress: Option<OnProgress>` last parameter `analyze_repo` already
  has. Every existing call site (tests included) is updated to pass
  `None`; behavior and output are unchanged when it is `None` — same
  breaking-but-additive signature change class this ADR's original
  Consequences section already accepted for `analyze_repo`.
- **UX**: `--base`/`--pr`/stdin modes under `--tui` now show a real gauge
  during "Analyzing diff..." instead of a static label, matching the
  whole-repo-outline and dependency-index phases. Every non-TUI display
  mode is unaffected, same as the original decision.
