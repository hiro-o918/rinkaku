# 0062. Confirm the update on the terminal before analysis, then re-exec

- Status: accepted
- Date: 2026-07-26
- Supersedes: parts of [ADR 0054](0054-tui-update-available-prompt.md) and
  [ADR 0056](0056-tui-update-prompt-auto-open-and-freeze-fix.md) (where and
  when the update is confirmed; the background check itself, the status-line
  hint, the `u` key, and the post-teardown `run_self_update(true)` call are
  unchanged)

## Context

ADR 0054 introduced a background version check spawned before analysis
and a `u`-triggered confirmation popup inside the TUI; ADR 0056 made
that popup auto-open the first time the check reports a newer release.
Both decisions put the confirmation *after* analysis, because that is
where the TUI lives.

Two measurements taken on this repository make that placement look
wrong:

- `run_analysis` on a typical `--base main` run takes roughly **1.9
  seconds**.
- `self_update::check_update_available` — one GitHub `/releases/latest`
  call plus a semver comparison — completes in roughly **0.07
  seconds**. Repeated sampling shows that is the typical case, not the
  worst: an occasional call takes ~0.3s (cold DNS/TLS) or even ~1.1s.
  The fallback path below exists precisely for those.

So the answer is already known before the analysis is a tenth of the
way through, and the reviewer is nevertheless made to wait ~1.9s and
then have a modal thrown over the entry screen. Worse, the two
decisions compound: choosing to update quits the TUI and discards the
analysis that was just paid for, and the reviewer has to re-run
`rinkaku` by hand afterwards to get back to where they were. The
expensive work is done first and then deliberately thrown away.

The confirmation is also drawn as a TUI popup only because of where it
sits in the sequence. Before `TuiSession::init` there is an ordinary
terminal available, and `run_self_update` already owns a perfectly good
y/N prompt for exactly this question.

## Decision

**Confirm the update on the ordinary terminal, before analysis starts,
and re-exec after a successful update.**

**Wait at most 300ms for the check.** The background thread and its
`mpsc` channel from ADR 0054 stay exactly as they are. Immediately
after spawning it, `main` does a single `recv_timeout(300ms)` on the
receiver. If a version arrives in time, the confirmation is offered
right there, on the terminal, before `TuiSession::init` is called. If
it does not, nothing is printed and the run proceeds straight to
analysis — the missed result is **deliberately tolerated**. The check
takes ~0.07s in the ordinary case, so 300ms is a ~4x margin over the
measured cost while still being short enough that a slow or flaky
network cannot make startup feel worse than it is today. Blocking
startup on a network call is precisely what ADR 0054's "never blocks
TUI startup" contract set out to avoid, and a bounded wait is the
smallest possible relaxation of it.

**The `u` key and the status-line hint remain the receiver for the
missed case.** The `Receiver` is still threaded into `TuiSession::run`,
so a check that lands after the 300ms window still reaches
`App::notify_update_available` on the event loop's poll tick, still
lights up the status-line hint, and is still reachable via `u`. This is
what makes tolerating the timeout acceptable rather than merely
lossy. When the confirmation *was* offered before analysis, the
receiver is not passed on (it has already been drained and answered),
so the TUI cannot ask the same question twice in one run.

**The TUI's auto-open is removed.** `should_auto_open_update_prompt`
and `App::update_prompt_dismissed` are deleted, and
`App::notify_update_available` goes back to only recording the version
(ADR 0054's original behavior). With the confirmation now offered
before the TUI exists, an auto-opening modal has nothing left to
justify it: ADR 0056's motivation was that the status-line hint was
missed for weeks, and a pre-analysis terminal prompt cannot be missed.
`update_prompt_open`, `update_requested`, and the popup's
`PopupConfirm`/`PopupCancel` handling all stay — the popup is still
reachable via `u`.

**An update that actually replaced the binary re-execs the same
command.** `main` replaces the current process image with the freshly
installed binary, passing `std::env::args_os()` through unchanged, so
the reviewer gets the analysis they asked for instead of a "please run
it again" message. On Unix this is
`std::os::unix::process::CommandExt::exec`; on non-Unix targets (none
are built today — see `build-and-publish.yaml`'s target list) the
`#[cfg(not(unix))]` fallback prints the same "updated, re-run rinkaku"
message the old flow effectively produced.

The re-exec is gated on the update having *happened*, not on
`run_self_update` merely having succeeded. `run_self_update` returns
`Ok` without updating on three paths (the version comparison finds
nothing newer, the confirmation is cancelled, and `updater.update()`
reports `updated() == false`), and re-execing on any of them would put
the *same* version back at `main`, where it would find the same release
newer and ask the same question again — a loop for as long as the user
keeps answering `y`. So `run_self_update` returns an `UpdateOutcome`
(`Updated` / `NotUpdated`) rather than `()`, and only `Updated` reaches
the `exec`; `NotUpdated` falls through to analysis exactly like a
declined prompt. The mapping is the pure `decide_after_update`, unit
tested for both outcomes.

**`exec` runs no destructors, so the deferred log sink is drained
explicitly before it.** ADR 0033's `--tui` branch buffers `log::`
records in a `DeferredLogSink` and relies on a `ReleaseGuard`'s `Drop`
as its safety net for abrupt exits. Replacing the process image skips
every `Drop` in the stack, that guard included, so anything buffered
during the pre-analysis check (`run_self_update` does network IO, and
its dependencies may log) would vanish silently. `main` therefore hands
`offer_pre_analysis_update` a `before_reexec` callback that releases
the sink, invoked on the accept-and-updated path only — the paths that
continue into the TUI must keep deferring until the alternate screen is
gone.

**stdin-piped input can never reach the re-exec.** `gh pr diff 123 |
rinkaku` consumes stdin on the way in; a re-exec'd process would find
that pipe already at EOF and analyze nothing. The guard is the same
`confirm_mode` rule ADR 0054's `self-update` subcommand already uses:
the pre-analysis confirmation is only offered when stdin is a TTY.
Piped input means stdin is not a TTY, so no confirmation is offered, so
no update runs, so no re-exec happens — the dangerous case is
structurally unreachable rather than specially cased. The decision
function `decide_pre_analysis_prompt` makes this explicit by taking
`stdin_is_tty` and returning `Skip` when it is false, and it is
unit-tested for exactly that. `RINKAKU_UPDATE_CHECK=0` continues to
skip the check (and therefore this whole path) entirely.

## Alternatives

- **Wait for the check to finish, however long it takes.** Rejected:
  makes every startup hostage to GitHub's latency and the user's
  connection. Today's design pays zero startup cost for the check; an
  unbounded wait would turn a best-effort hint into a hard dependency,
  and a 5-second stall before anything is drawn is a far worse regression
  than occasionally falling back to the status-line hint.
- **Just remove ADR 0056's auto-open, keep the confirmation in the
  TUI.** Rejected: it fixes the interruption but not the waste. The
  reviewer still spends ~1.9s on an analysis that confirming the update
  discards, and still has to re-run by hand. The auto-open is a symptom
  of the confirmation being in the wrong place, not the disease.
- **Keep the confirmation after analysis but re-exec instead of
  exiting.** Rejected: it removes the manual re-run but doubles the
  analysis cost — the discarded ~1.9s is still paid, and then paid
  again by the re-exec'd process.
- **Prompt before analysis but skip the re-exec, printing "updated,
  re-run rinkaku".** Rejected: this is what the `#[cfg(not(unix))]`
  fallback does, and it is strictly worse on the platforms that can do
  better. The reviewer's intent (analyze this diff) is fully known at
  that point, and `args_os()` is exactly the information needed to
  honour it.
- **Prompt even when stdin is not a TTY, reading the answer from
  somewhere else.** Rejected: there is nobody to answer, and the stdin
  pipe is very likely carrying the diff itself. Refusing to prompt is
  both the safe default and, as noted above, the mechanism that makes
  the re-exec's stdin problem unreachable.

## Consequences

- `rinkaku/src/update_prompt.rs` is a new module holding the pure
  decision (`decide_pre_analysis_prompt`) and the re-exec's argument
  handling; the IO around it (channel receive, terminal read,
  `exec`) stays in that module's thin boundary functions and `main.rs`.
- `main.rs`'s `DisplayMode::Tui` branch gains a pre-analysis block
  between spawning the check and `TuiSession::init`; the
  `update_check` receiver it passes to `session.run` becomes `None`
  when the question has already been answered.
- `App` loses one field (`update_prompt_dismissed`) and the crate loses
  one free function (`should_auto_open_update_prompt`);
  `notify_update_available` reverts to a plain setter. Tests asserting
  the auto-open behavior are rewritten to assert the popup stays
  closed, which is what ADR 0054's original tests asserted.
- `run_self_update`'s own confirmation is untouched — the pre-analysis
  prompt is a separate, differently-worded question asked at a
  different time, and the subsequent `run_self_update(true, ..)` call
  still skips the redundant second prompt exactly as ADR 0054 arranged.
  Its signature does change: it takes an `Announcement` (so the
  pre-analysis caller, which already printed the version, can suppress
  the duplicate "New release found" line) and returns `UpdateOutcome`.
  The `self-update` subcommand and the TUI's `u` path both pass
  `Announcement::Print` and ignore the outcome, keeping their output
  and behavior identical.
- `is_affirmative` is shared rather than duplicated: `update_prompt`
  calls `self_update`'s, so there is one rule for what counts as
  consent to replace the running binary.
- No output format changed. `docs/tui.md`, `docs/cli.md`, and
  `README.md` are updated to describe the new confirmation point; the
  `?` help overlay's `U` row is unchanged, since the key still does the
  same thing.
- A user on a slow connection sees the pre-analysis prompt less often
  and the status-line hint more often. That is the accepted trade, and
  the 300ms number is the knob to revisit if it turns out to be too
  tight in practice.
