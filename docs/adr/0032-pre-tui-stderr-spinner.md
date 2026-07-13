# 0032. A stderr spinner during pre-TUI analysis

- Status: accepted
- Date: 2026-07-13

## Context

Every input mode `rinkaku` supports runs a synchronous analysis pipeline
before producing any output at all: `analyze_repo` for the whole-repo
outline, `build_resolver` + `analyze_diff` for stdin-piped diffs, and
`run_git_diff` + `build_resolver` + `analyze_diff` for `--base`/`--pr`.
ADR 0031's profiling table shows this can take anywhere from tens of
milliseconds (a small repo, `--deps 0`) to multiple seconds (a large
repository's dependency index, or a whole-repo outline on a big tree).
For the whole of that window, the terminal shows nothing — no output,
no cursor movement, nothing — so the process looks hung, particularly
right before `--tui` (the default when stdout is a terminal, ADR 0017)
opens the alternate screen with its own more informative loading state.

`main.rs` already has some feedback here via `log::info!` lines
(`env_logger`'s default is `info` level, per `main`'s own comment), but
those are one-shot, print-once log lines tied to specific milestones
(`"diffing {base}...{head}"`, `"building dependency index over N
files"`, `"analyzing diff"`) — there is no continuous signal that the
process is alive *between* those lines, and a long-running phase (e.g.
tree-sitter parsing thousands of files in `analyze_repo`, or indexing a
large repository's dependency graph in `build_resolver`) still produces
a silent gap.

ADR 0031's Alternatives section already named two options for
addressing perceived latency around this same pipeline — progressive
TUI rendering and lazy start — and explicitly deferred both as "a
larger architectural change to `rinkaku-tui`'s state machine". This ADR
is not a revisit of that decision: a spinner does not compete with
either alternative, it addresses a narrower problem (visible liveness
during a wait, not reduced wait time or earlier interactivity) with a
much smaller change.

## Decision

Show an indeterminate spinner on stderr for the whole duration of the
pre-output analysis phase, across every input mode. The spinner's
message updates as the pipeline moves between phases (resolving a
`--pr` argument, diffing, building the dependency index, parsing the
repository, analyzing the diff), and is cleared
(`ProgressBar::finish_and_clear`) before either the `Report` is handed
to `--entry`'s pivot step or any output (Markdown/JSON/TUI) is
produced — in particular, before `rinkaku_tui::run` enters the
alternate screen, so no stray spinner line is left behind for the TUI's
first frame to render over.

Implementation:

- New `indicatif = "0.17"` workspace dependency, consumed only by the
  `rinkaku` bin crate (`rinkaku-core` and `rinkaku-tui` stay untouched —
  `rinkaku-core` because CLAUDE.md's pure-core rule forbids IO there,
  `rinkaku-tui` because rendering progress before the TUI even starts is
  not the TUI's concern). `indicatif` was already present in
  `Cargo.lock` as a transitive dependency of `self_update` (same
  version, 0.17.11), so this adds no new major dependency to the
  build — only promotes an existing transitive dependency to direct.
- A new `rinkaku/src/spinner.rs` module: a thin `Spinner` wrapper around
  `indicatif::ProgressBar` (`start`/`set_message`/`finish_and_clear`),
  plus a pure `AnalysisPhase` enum and `phase_message` function mapping
  each phase to its stderr text — kept pure and unit-tested
  (`rstest` + `pretty_assertions`) per this project's IO-boundary
  testing convention. The `indicatif`-touching `Spinner` type itself is
  kept thin (no branching logic beyond forwarding to `ProgressBar`) and
  left untested, per CLAUDE.md's "no mocking of external processes"
  test strategy — there is no real terminal to assert against in a unit
  test, so the pure `phase_message` split is what carries the test
  coverage instead.
- `main()` starts one `Spinner` right after the `SelfUpdate` early
  return and threads a `&Spinner` through `run_base_pipeline` and
  `build_resolver` so each can update the message as its own sub-phase
  starts, mirroring the existing `log::info!` call sites at each of
  those same milestones.

An early `?`-propagated error (e.g. a failing `git`/`gh` subprocess call
inside `run_base_pipeline`/`build_resolver`) drops the `Spinner` before
`finish_and_clear()` is ever reached. This is not a gap: `indicatif`'s
`BarState` clears the line on `Drop` unless the bar was already finished,
using `ProgressFinish::AndClear` — the crate's documented default — so a
spinner is guaranteed to be cleared from the terminal one way or another
before the process's error message reaches stderr.

Non-TTY stderr (piped, redirected, CI) needs no special-casing:
`indicatif`'s `ProgressDrawTarget::stderr()` (the default target
`ProgressBar::new_spinner()` uses) wraps `console::Term`, which detects
whether the underlying stream is a real terminal and suppresses all
drawing when it isn't — verified both by reading `draw_target.rs`'s own
doc comment ("if the terminal is not user attended the entire progress
bar will be hidden") and empirically: `rinkaku --base HEAD~1 --format md
2>err.log` produces zero ESC bytes in `err.log`, and a `script`-driven
pseudo-TTY session shows the same run redrawing the spinner in place
(`\x1b[2K` clears) through each phase and then clearing cleanly right
before the Markdown/TUI output starts.

## Alternatives

- **Do nothing / rely on `log::info!` lines alone**: cheapest, but
  leaves the exact gaps described above — a phase with no further log
  line until it finishes (large `analyze_repo`/`build_resolver` runs)
  still looks hung for its whole duration.
- **Progressive TUI rendering / lazy start** (ADR 0031's deferred
  alternatives): solve a different problem — reducing time-to-first-
  interaction rather than making an unavoidable wait visible — and
  remain a substantially larger change to `rinkaku-tui`'s state
  machine. Not superseded by this ADR; still open for a future PR if
  time-to-interaction itself needs to improve.
- **A hand-rolled spinner** (raw ANSI escapes + a background thread):
  avoids the new dependency, but reimplements TTY detection, frame
  timing, and line-clearing that `indicatif` already gets right — not
  justified when `indicatif` is already in the dependency tree
  transitively.
- **Emit periodic `log::info!` heartbeat lines instead of a spinner**:
  simpler, no new dependency, but produces scrolling log noise on a
  real terminal instead of a single redrawn line, which is a worse
  experience for the common case (an interactive user watching the
  terminal) to optimize for the uncommon one (structured log capture).

## Consequences

- **Dependencies**: `rinkaku` (the bin crate) gains a direct dependency
  on `indicatif`, declared at the workspace level like every other
  shared dependency. No change to `rinkaku-core`'s or `rinkaku-tui`'s
  dependency sets. `Cargo.lock` is otherwise unchanged (`indicatif` was
  already resolved to 0.17.11 via `self_update`).
- **UX**: every input mode now shows continuous stderr feedback during
  analysis, not just milestone log lines. On a non-TTY stderr (CI,
  redirected output), behavior is unchanged — no spinner bytes are
  written, so existing scripts parsing `2>` output are unaffected.
- **Testing**: `phase_message`'s phase → text mapping is unit-tested as
  a pure function. `Spinner` itself is not unit-tested (same rationale
  CLAUDE.md gives for not mocking external processes: it is a thin IO
  wrapper with no branching logic of its own to exercise). Dynamic
  verification (this PR's body) covers the non-TTY silence guarantee
  and the TTY draw/clear sequence directly, since neither is expressible
  as a unit test without a real terminal.
- **API surface**: `run_base_pipeline` and `build_resolver` (both
  private to `main.rs`) gain a `&Spinner` parameter; their existing test
  call sites in `main.rs`'s own test module were updated to pass a
  `Spinner::start(...)` instance. No change to any public crate API.
- **Error-path cleanup**: no explicit `finish_and_clear()` call is needed
  on any early-return error path — `indicatif`'s `Drop` impl for
  `BarState` clears the line by default, so an early `?` return leaves
  no visual artifact on stderr either.
