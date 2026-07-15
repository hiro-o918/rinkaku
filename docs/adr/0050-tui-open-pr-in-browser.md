# 0050. TUI: open the PR in a web browser

- Status: Accepted
- Date: 2026-07-15

## Context

`--pr` mode already resolves everything needed to identify the PR being
reviewed (`PrContext { owner, repo, number, head_sha }`, introduced by
ADR 0048 for sink A's `gh api` calls). A reviewer working through the
TUI who wants to jump to the PR's page on github.com — to check CI
status, read an existing comment thread, or hand the URL to someone
else — currently has to reconstruct the URL by hand or shell out
separately; there is no in-TUI action for it, unlike `gh pr view -w`'s
familiar one-flag shortcut.

## Decision

**A new global key, `w`/`W`, opens the current PR's page in the
reviewer's default web browser.** `w` is chosen to match `gh` CLI's own
`-w`/`--web` convention for "open in a browser"; `o`/`O` (`ToggleOrder`)
and `s`/`S` (`Source`) are already bound, and `g` is reserved as the
`gd`/`gr`/`gg` two-key prefix, so none of those are available.

**Scope is the PR page URL only**: `https://github.com/{owner}/{repo}/pull/{number}`.
No cursor-relative URL (a file/line-anchored link into the diff) is
built — see Alternatives.

**Port**: a new `BrowserOpener` trait, one method, defined in
`rinkaku-tui` (the consumer side) alongside `ReviewSubmitter`/
`ClipboardSink`:

```rust
pub trait BrowserOpener {
    fn open_url(&self, url: &str) -> Result<(), String>;
}
```

The `rinkaku` binary crate implements it by spawning the platform's
"open a URL" command directly (`open` on macOS, `xdg-open` on Linux),
mirroring `clipboard.rs`'s existing direct-spawn shape (ADR 0048's own
clipboard sink) rather than adding a crate dependency for a single OS
command invocation. `main.rs`'s composition root constructs the
implementation and wires it in, the same way `SystemClipboard`/
`GhReviewSubmitter` are wired today.

**Availability**: `w` is a global key (translated regardless of
screen/focus, like `d`/`r`/`s`), but the action it performs needs a
`PrContext` — unavailable outside `--pr` mode (stdin input, `--base`
mode). Rather than hiding the key (impossible for a single global
binding the way an overlay's own menu can omit an entry, ADR 0048's
sink A precedent) or silently no-op'ing, pressing `w` without a
`PrContext` sets a status-line message explaining why, mirroring the
existing "note: jumplist has no earlier location"-style status-line
feedback `App::handle_key`'s `JumpBack`/`JumpForward` arms already give
for an unavailable action. A spawn failure (no `open`/`xdg-open` on
`PATH`, or the browser command itself erroring) is reported the same
way — best-effort, never a crash.

**Wiring**: `PrContext` is already threaded from `main.rs` through
`ReviewPorts` down to `run_app` (ADR 0048); this feature adds one more
field to that same struct (`browser: &dyn BrowserOpener`) rather than a
second parallel plumbing path. Because `App` itself does not hold
`PrContext` (only `review_sink_a_available: bool`, ADR 0048's own
"App doesn't need the whole context, just whether sink A is on the
menu" decision), `w` is special-cased in `run_app` before dispatch —
the same pattern `InputKey::NoteCompose`/`InputKey::Source` already
follow for "needs data `App::handle_key` doesn't have."

## Alternatives

- **Build a cursor-relative URL** (a link into the diff anchored at the
  row/line under the cursor, using GitHub's `#diff-<hash>` file-anchor
  or line-range fragment). Rejected for v1: the TUI's own data
  structures have no notion of "a GitHub URL for this location" today
  (`review::NoteLocation`'s anchor is a 1-based line range, not a
  file-content hash GitHub's own `#diff-` fragments require), and
  computing that hash correctly would add a new dependency (a SHA-256
  implementation, unlike ADR 0048's base64, which this project already
  hand-rolls) for a feature this ADR's scope does not need. The PR page
  itself is one click away from any file's diff via GitHub's own UI, so
  the marginal benefit of a deep link is small relative to the added
  surface. A future ADR can revisit this once a concrete reviewer need
  is identified.
- **Silently no-op when `PrContext` is unavailable**, instead of a
  status-line message. Rejected: indistinguishable from the key simply
  not being bound, which is worse for discoverability than the
  existing jumplist-empty precedent this ADR follows — a reviewer who
  presses `w` outside `--pr` mode should learn why nothing happened,
  not wonder if they mistyped.
- **Add a crate dependency (e.g. `open`) for cross-platform URL
  opening** instead of spawning `open`/`xdg-open` directly. Rejected on
  the same grounds ADR 0048 rejected `arboard` for clipboard access:
  this project already has a working direct-spawn precedent
  (`clipboard.rs`) for exactly this class of "shell out to a
  platform-specific helper program" problem, and a one-method port
  needs no crate to satisfy it.

## Consequences

- `ReviewPorts` gains one field (`browser: &dyn BrowserOpener`),
  always present (unlike `submitter`, which is `Option`) since the key
  itself is global — the port always exists, only the `PrContext` it
  needs may be absent.
- `InputKey` gains one variant (`OpenPrInBrowser`), translated
  unconditionally by `input_translate::translate_key` like every other
  global key, and special-cased in `run_app` before dispatch (mirrors
  `NoteCompose`/`Source`'s existing precedent) rather than routed
  through `App::handle_key`.
- `rinkaku`'s composition root gains one more concrete port
  implementation (`SystemBrowserOpener` or equivalent), tested the same
  way `clipboard.rs`'s `copy_via_command` is — a nonexistent-command
  spawn-failure case, not a real browser launch.
- `docs/tui.md`'s Global key-bindings table and the `?` help overlay's
  Global group both gain a `w` row; `README.md`'s "How to read the
  TUI" section gains one line describing the action (no ADR reference
  in that file, per this project's convention).
