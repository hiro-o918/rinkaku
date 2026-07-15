# 0054. TUI: prompt to self-update when a newer release is available

- Status: Accepted
- Date: 2026-07-15

## Context

`rinkaku self-update` (the bin-crate `self_update.rs` module) already
lets a user pull the latest GitHub release in place, but only when they
remember to run it — no ADR covers that pre-existing design (it
predates this project's ADR-per-structural-decision convention), so
this ADR's Decision also records it in passing: version-check/download
IO lives in the `rinkaku` bin crate, not `rinkaku-core`, because it is
tied to how *this specific binary* is distributed, the same "IO at the
boundary" reasoning ADR 0047 already applies to the TUI's own
`SourceReader` port. A `--tui` session is exactly the setting where a
user is least likely to remember to check: they are mid-review, not
running one-off CLI commands, so the update simply goes unnoticed for
weeks.

## Decision

**A background version check runs on every `--tui` startup**, opt-out
via `RINKAKU_UPDATE_CHECK=0`. `main.rs`'s composition root spawns a
detached `std::thread` before `TuiSession::init`, which calls a new
`self_update::check_update_available() -> Option<String>` — extracted
from `run_self_update`'s existing `Update::configure()` builder
(`build_updater`, shared by both entry points so they cannot drift) —
and sends the version string over a `std::sync::mpsc::channel` only
when `self_update::version::bump_is_greater` says the release found is
actually newer. The thread is never joined; any failure (network,
GitHub API, an unparsable version) is silently swallowed, matching
`run_self_update`'s own network-call error handling but without a
propagation path — this is a best-effort background hint, not a
user-initiated action with something to report failure to.

**`rinkaku-tui` receives only plain data over the channel — no
network/process awareness of its own.** The `Receiver<String>` is
threaded through `TuiSession::run` and `run_app` as
`Option<std::sync::mpsc::Receiver<String>>`; the event loop's existing
100ms poll tick does a non-blocking `try_recv()` and, on a message,
calls `App::notify_update_available(version)`. This keeps the crate's
"terminal adapter is the only IO layer, `app`/`row_view`/etc. stay
pure" split (`lib.rs`'s own module doc comment) intact: the channel is
IO-shaped, but `App` only ever sees a `String` it did not fetch itself.

**The status line shows a persistent hint** (`update vX.Y.Z: U`,
`ui/status.rs`) once `App::update_available()` is `Some` — appended
after the file-size-warning suffix, the same "skipped when `None`"
shape that suffix already has. Persistent, not the transient
`App.status` slot: the hint must survive the very next handled key,
unlike a one-shot status message.

**The popup never auto-opens.** `App` gains an `update_prompt_open`
flag (mirroring `jump_popup`'s own "sits on top of whatever was
already showing" shape) that only a new global key, `U`, can set — and
only when `update_available` is already `Some`; otherwise `U` is a
no-op. An unprompted modal stealing keystrokes mid-review the moment a
background thread happens to finish would be a materially worse
experience than a quiet status-line hint the reviewer acts on when
ready — the same reasoning that keeps every other background/derived
state in this crate (blast-radius selection, diff-pane content)
recomputed on demand rather than pushed onto the screen uninvited.

**Confirming quits the TUI; the update itself runs after teardown.**
`InputKey::PopupConfirm` while the prompt is open sets both
`should_quit` and a new `update_requested` flag; `run_app` now returns
`std::io::Result<bool>` (the bool is `update_requested`) instead of
`std::io::Result<()>`, and `TuiSession::run` passes that bool through
unchanged, *after* its own unconditional terminal-restoring postamble
has already run. `main.rs` checks the returned bool once the TUI call
has returned and, if set, calls the existing `run_self_update(true)` —
`yes: true` because the reviewer already confirmed inside the TUI's own
popup, so the CLI-side confirmation prompt (`ConfirmMode::Prompt`)
would be redundant. This ordering — terminal restored, then the
download runs on an ordinary scrollback — reuses `run_self_update`
unchanged rather than teaching it to run mid-alternate-screen.

**Key choice**: `U` (uppercase). `u`/`U` were both free (no existing
`InputKey` binds either); this ADR binds both cases to the same
variant, matching every other single-letter global key in this crate
(`d`/`D`, `w`/`W`, `s`/`S`) rather than reserving the lowercase form for
a future action.

## Alternatives

- **Auto-open the popup the instant the background thread reports a
  version.** Rejected: see Decision above — an unprompted modal is a
  worse interruption than a persistent, ignorable status-line hint.
- **Periodic re-check** (poll again every N minutes instead of once at
  startup). Rejected as unnecessary scope: a `--tui` session is
  typically a single review sitting, not a long-running daemon: one
  check per invocation is enough, and a periodic timer would add retry/
  backoff concerns this ADR has no need to solve.
- **Return a dedicated outcome struct from `TuiSession::run`/`run_app`**
  instead of `std::io::Result<bool>`. Considered for future
  extensibility (a struct scales better if a second post-quit action
  is ever added), but rejected for now: `update_requested` is the only
  thing any caller needs out of this call today, and `bool` says
  exactly that with no indirection; a struct wrapper can be introduced
  later without changing today's call sites' logic if a second flag
  ever appears.
- **Have the TUI crate call `self_update` directly** (skip the
  channel, let `rinkaku-tui` hold a copy of the check function).
  Rejected: `rinkaku-tui` performs exactly one narrowly-scoped IO
  operation today (the source drill-down's file read, itself behind
  the `SourceReader` port per ADR 0047) — adding a GitHub API client on
  top would cross the same "IO isolated to `rinkaku`'s adapter layer"
  line ADR 0016/0047 already draw, for a capability the bin crate
  already has.

## Consequences

- `rinkaku/src/self_update.rs` gains `check_update_available()` and a
  shared `build_updater()` helper; `run_self_update` is unchanged in
  behavior, only refactored to call the shared helper.
- `rinkaku-tui` gains no new dependency — the channel is
  `std::sync::mpsc`, already in `std`.
- `App` gains three fields (`update_available`, `update_prompt_open`,
  `update_requested`) and one setter (`notify_update_available`); every
  transition is unit-testable without a live terminal, matching this
  crate's existing `App`-is-pure discipline.
- `run_app`'s and `TuiSession::run`'s signatures both change: one new
  `Option<Receiver<String>>` parameter, and the return type widens from
  `std::io::Result<()>` to `std::io::Result<bool>`. The crate's free
  `run()` convenience wrapper (no version-check thread of its own)
  passes `None` and discards the bool, preserving its own
  `std::io::Result<()>` signature for any caller that does not need the
  update flow.
- `docs/tui.md`'s Global key-bindings table and the `?` help overlay's
  Global group both gain a `U` row; `docs/cli.md`'s `self-update`
  section documents the TUI prompt and the `RINKAKU_UPDATE_CHECK=0`
  opt-out; `README.md`'s install section gains a few words pointing at
  the `U` key (no ADR reference in that file, per this project's
  convention).
- A future second post-quit action (if one is ever needed) will want to
  revisit the `bool` return in favor of a small outcome struct — noted
  in Alternatives, not attempted here.
