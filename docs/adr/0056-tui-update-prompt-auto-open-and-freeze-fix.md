# 0056. TUI: auto-open the update prompt at startup, fix its dead keys, and stop the silent self-update stall

- Status: Accepted
- Date: 2026-07-16

## Context

ADR 0054 shipped a status-line hint plus a `u`-triggered confirmation
popup for the TUI's background update check, deliberately choosing
**not** to auto-open the popup: "an unprompted modal stealing keystrokes
mid-review ... would be a materially worse experience than a quiet
status-line hint." In practice, users did not notice the status-line
hint (it sits in the bottom-right corner, easy to miss while focused on
a diff), so the update went unnoticed for weeks at a time — defeating
the whole feature's purpose.

Separately, a user reported that pressing `u` and confirming appeared to
**freeze the app** on v0.6.11. Investigating end-to-end (version-check
thread, `App`/`run_app` dispatch, `TuiSession`'s terminal-lifecycle
postamble, and `rinkaku/src/self_update.rs`'s use of the `self_update`
crate) found two independent, real defects, plus confirmed one part of
the design already correctly guards against the classic version of this
bug:

1. **A real dead-key bug**: `rinkaku-tui/src/input_translate.rs`'s
   `translate_key` had no short-circuit for
   `App::update_prompt_open()`, unlike every other modal in this crate
   (the `?` help overlay, the jump-target popup, the review overlay all
   have one). While the update popup was open, Enter translated to
   `InputKey::Open` (not `PopupConfirm`), Esc translated to `None` or
   `FocusLeft`, and `q` translated to `InputKey::Quit` — none of which
   `App::handle_key`'s `update_prompt_open` branch recognizes (it only
   reacts to `PopupConfirm`/`PopupCancel`, treating everything else as
   the intentional confirm/cancel `_ => {}` no-op every other modal in
   this crate also uses). The popup's own two hint labels ("[Enter]
   update & quit [Esc] not now") were lies: pressing either key did
   nothing at all. This is confirmed the primary cause of the reported
   freeze — the popup simply never responded to the keys its own hint
   text told the user to press, which is indistinguishable from a hang
   without reading the dispatch code. Existing tests never caught this
   because `rinkaku-tui/src/app/tests/update_prompt.rs` only ever
   constructed `InputKey::PopupConfirm`/`PopupCancel` directly, bypassing
   `translate_key` entirely — the same class of gap ADR 0022's
   `pending_prefix` regression already taught this crate to distrust
   (`event_loop::dispatch_non_source_key`'s own doc comment).
2. **A real, if less severe, silent-stall gap**: once the update *does*
   run (after the terminal is correctly restored — see below),
   `run_self_update`'s builder sets `.show_output(false)` (deliberately,
   to suppress the `self_update` crate's own misleading "*NOT*
   compatible" line — see that function's existing doc comment). This
   also suppresses every intermediate message inside the crate's
   `update_extended()`, including a **second**, independent
   `/releases` API call it makes internally (in addition to the one
   `run_self_update` already made to decide whether an update exists at
   all) and the "Downloading..."/"Replacing binary file..." lines. On a
   slow connection, or when the release asset's response has no
   `Content-Length` header (no progress bar at all in that case), the
   window between "New release found: ..." and the final "Updated ..."
   line is completely silent — no spinner, no dots, nothing — which
   looks exactly like a hang.
3. **Already correct, not a bug**: `main.rs`'s composition root already
   defers the actual `run_self_update(true)` call until after
   `TuiSession::run`'s terminal-restoring postamble has completed (its
   own doc comment: "the update itself runs only after the block above
   has already restored the terminal"), and the version-check thread is
   a detached, never-joined `std::thread::spawn` whose result reaches
   `App` only via a non-blocking `try_recv()` on the existing 100ms poll
   tick. Neither of those — the classic "blocking IO while the
   alternate screen/raw mode is still active" freeze shape — was
   present. This ADR's fixes are additive to that existing design, not a
   rewrite of it.

## Decision

**Keep in-app self-update.** Both defects found are concrete and fixable
without touching the `self_update` crate's fundamentally synchronous
API or `main.rs`'s existing "update runs after teardown" ordering, so
there is no need to fall back to a guidance-only prompt.

**Fix 1: `translate_key` gains an `App::update_prompt_open()`
short-circuit**, mirroring the jump-target popup's own structure
immediately above it: Enter maps to `PopupConfirm`, Esc/`q` map to
`PopupCancel`, every other key is swallowed. Placed after the jump-popup
check (the two popups can never be open together, per
`InputKey::OpenUpdatePrompt`'s own gating) and before the ordinary
entry-view key matching, so no future key mapping can silently reach
past it the way Enter/Esc/`q` did before this fix.

**Fix 2: `run_self_update` wraps the `updater.update()` call in the
existing `Spinner`** (`rinkaku/src/spinner.rs`, already used for the
non-TUI analysis pipeline's own "long synchronous call, no feedback"
problem) rather than flipping `show_output(true)` back on, which would
reintroduce the misleading "*NOT* compatible" line
`build_updater`'s doc comment already explains rejecting.
`Spinner::start`/`finish_and_clear` bracket the one call that can take
an unbounded, unpredictable amount of wall-clock time silently.

**Change: the update popup now auto-opens** the first time
`App::notify_update_available` is called, unless the reviewer already
dismissed it once this session — reversing ADR 0054's "popup never
auto-opens" decision now that the two defects above are fixed. The
original worry (an unprompted modal stealing keystrokes mid-review) is
judged an acceptable trade against the status-line hint's demonstrated
failure mode (silently ignored for weeks). A new `App` field,
`update_prompt_dismissed`, tracks whether `PopupCancel` has already
closed the popup this session; a free function,
`should_auto_open_update_prompt(update_available: bool,
update_prompt_dismissed: bool, no_other_modal_active: bool) -> bool`,
decides this and is deliberately not a method on `App` — a backlogged startup splash screen
is expected to want the identical "show once, don't reopen after an
explicit dismissal" decision, and a free function taking plain `bool`s
can be reused there without depending on `App`'s internals. The
status-line hint and `u`-to-reopen both keep working unchanged after a
dismissal — only the very first appearance is now automatic. An
already-open modal (the help overlay or the jump popup) takes priority:
`notify_update_available` will not auto-open the update prompt over
one, since the renderer draws the update prompt topmost while
`translate_key` still routes keys to whichever modal it checks first,
and the two would otherwise disagree about what is actually receiving
input. The reviewer still reaches the popup via `u` after closing the
other modal.

## Alternatives

- **Drop in-app update execution, guidance-only prompt.** Rejected: both
  root causes found are narrowly-scoped, low-risk fixes (a missing
  `match` arm in `translate_key`, a spinner around one already-isolated
  call), not evidence of a structurally unfixable synchronous API. The
  user's own stated policy is to keep in-app execution when the freeze
  is fixable.
- **Flip `show_output(true)` back on to silence Fix 2's gap.** Rejected:
  reintroduces the `self_update` crate's own "*NOT* compatible" line for
  any ordinary pre-1.0 minor bump, which `build_updater`'s existing doc
  comment already documents as actively misleading for this project.
- **Skip the second, redundant `/releases` call inside
  `updater.update()`** by teaching `run_self_update` to reuse the
  `Release` it already fetched via `get_latest_release()`, rather than
  letting `update()` re-fetch it. Rejected for this PR: `update()`'s
  `Result<Status>` return does not expose an entry point that accepts an
  already-fetched `Release` (the crate always re-resolves the release
  list itself), so avoiding the second call would mean re-implementing
  `update_extended()`'s download/extract/replace sequence by hand
  instead of calling the crate's own `update()` — a materially larger,
  riskier change than wrapping the existing call in a spinner. Noted as
  a possible follow-up if the crate ever adds a "confirm this pre-fetched
  release" entry point.
- **Auto-open only on the very first frame after the version-check
  thread reports, versus on every `notify_update_available` call.**
  Equivalent in practice: `main.rs`'s channel only ever sends one
  message per session (the check thread runs once at startup and exits),
  so `notify_update_available` is itself only ever called once — the
  `update_prompt_dismissed` guard is what actually matters, guarding
  against a hypothetical future caller that notifies more than once.

## Consequences

- `rinkaku-tui/src/input_translate.rs`'s `translate_key` gains one new
  short-circuit block; no signature change.
- `rinkaku-tui/src/app/mod.rs`'s `App` gains one new field
  (`update_prompt_dismissed`) and one new free function
  (`should_auto_open_update_prompt`); `notify_update_available`'s
  behavior changes from "never opens the popup" to "opens it once,
  unless already dismissed this session or another modal is currently
  open" — every existing test that asserted the old "does not
  auto-open" behavior was updated to dismiss the popup first where that
  state is still what the test needs to reach.
- `rinkaku/src/self_update.rs`'s `run_self_update` gains a `Spinner`
  around `updater.update()`; behavior is otherwise unchanged (same
  network calls, same final messages).
- No public output format changed; no other crate's contract changed.
- `docs/tui.md`/the `?` help overlay's `U` row text is unaffected — the
  keys and their meaning are unchanged, only their previously-broken
  wiring is fixed and the popup's opening trigger widens from
  `u`-only to `u`-or-automatic-on-first-availability.
