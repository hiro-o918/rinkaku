# 0026. TUI scrolling: reviewer-controlled motion in the source screen and the entry-view right pane

- Status: accepted
- Date: 2026-07-13

## Context

Two closely related complaints came out of dogfooding, addressed
together because they share one root cause and one fix.

**Source screen (`s` key, ADR 0015/0016).** `Screen::Source` reads the
selected symbol's file from disk and shows a window centered on its
definition (`crate::source::visible_window`,
`crate::ui::draw_source_screen`). `App::handle_key`'s `Screen::Source`
arm is deliberately minimal: `Back` returns to the entry view and every
other key is a no-op — the pre-existing
`should_return_none_from_draw_when_the_source_screen_is_open` test in
`crate::ui` even asserts "the source screen scrolls via its own
auto-centering window". This is the wrong end of the design space. The
symbol under the cursor is the *entry point* the reviewer picked to
read from, not the entirety of what they want to read: the caller a few
lines above, the helper a few lines below, the imports at the top of
the file, are all in scope for "understand this change" and none of
them are reachable today without leaving the TUI.

**Entry-view right pane on `Focus::Right` (ADR 0020).** `j`/`k` already
scroll the right pane by one line (Diff, Detail, and BlastRadius all
share `App::right_pane_scroll`), but there is no half-page motion and
no top/bottom jump — for a long diff pane in particular, reaching the
end takes a great many keypresses that vim's `Ctrl-d`/`G`
equivalents would collapse into one.

Both surfaces need the same three motion primitives (`j`/`k` is already
in place on the entry-view side): half-page down/up and jump to
top/bottom, with viewport-height-aware step sizes so the same key does
the right thing on both a 6-line and a 60-line pane. The state
discipline already exists once (`App::right_pane_scroll` as an
unclamped request, `crate::ui::clamp_scroll` at draw time, fold the
clamped value back via `App::with_right_pane_scroll` after every
draw — `crate::lib::run_app`'s `clamp_right_pane_scroll_after_draw`);
this ADR extends that same pattern rather than introducing a second
mechanism.

## Decision

**1. `Screen::Source` carries a `scroll_top` line offset.** Change
`Screen::Source { symbol_id: String }` to `Screen::Source { symbol_id:
String, scroll_top: usize }` — 0-based, "first visible line", same
shape `right_pane_scroll` uses. `scroll_top` is an unclamped *request*:
`crate::ui::draw_source_screen` clamps it against the file's actual
line count and the pane's rendered height at draw time, matching the
`right_pane_scroll` clamp-at-draw discipline.

**2. Initial `scroll_top` is `visible_window`'s centered start.** When
`s` opens `Screen::Source`, `crate::run_app` computes
`visible_window`'s start line (the same computation
`draw_source_screen` performs today per frame, called once here at
transition time) and stores it in `scroll_top`. The very first frame
still shows the same centered window it does today; subsequent motion
keys scroll away from that starting position rather than fighting an
auto-recentering.

**3. Four new `InputKey` variants** — `ScrollHalfPageDown`,
`ScrollHalfPageUp`, `ScrollToTop`, `ScrollToBottom`. Named for what
they do rather than the physical key (`InputKey::Down` already
precedes: it is not `InputKey::J`), so key-to-intent mapping stays in
`translate_key`. Bound identically on both surfaces:

- **On `Screen::Source`**: acts on `Screen::Source.scroll_top`.
- **On `Screen::Entry` + `Focus::Right`**: acts on
  `App::right_pane_scroll`, whichever right pane
  (`Diff`/`Detail`/`BlastRadius`) is currently showing — the same
  "j/k scrolls whatever the right pane is showing" rule ADR 0020
  already established, extended to these three additional motions.
- **On `Screen::Entry` + `Focus::Tree`**: no-op. Consistent with how
  today's `j`/`k` do not scroll the right pane while Tree-focused.

**4. Key bindings** — vim-idiomatic, chosen to interlock with the
existing `g`-prefix state machine (ADR 0022) rather than fight it:

- `Ctrl-d`: `ScrollHalfPageDown`
- `Ctrl-u`: `ScrollHalfPageUp`
- `gg`: `ScrollToTop` — resolved by `translate_key` the same way
  `gd`/`gr` are: when `App::pending_prefix()` is already `Some(G)` (set
  by an earlier `g` press) and a second `g` arrives, emit
  `ScrollToTop`. A single `g` press remains `InputKey::PendingGoto` as
  today; `gd`/`gr` continue to resolve to
  `GotoDefinition`/`GotoReferences` unchanged. `gg` is a strict addition
  to the resolver table.
- `G` (`Shift-g`): `ScrollToBottom`.

`gg` is chosen over single-key `g` even on `Screen::Source` (which has
no `gd`/`gr` to collide with) so the same two-key gesture works on both
screens — reviewers do not have to relearn "top of pane" per screen.

**5. Half-page step needs the viewport height.** `App::handle_key`
does not know the pane's rendered height (a `ratatui::Rect` only
`crate::ui` sees), so `ScrollHalfPageDown`/`ScrollHalfPageUp` cannot
be handled by `App::handle_key` alone. `crate::ui::draw` already
returns `Option<usize>` (the clamped right-pane scroll for
`clamp_right_pane_scroll_after_draw`); it grows to also surface the
last-drawn inner height of the currently-scrolling pane (source pane
on `Screen::Source`, right pane on `Screen::Entry` + `Focus::Right`,
`None` otherwise). `crate::run_app` remembers this height between
frames and passes it into a new `App::handle_scroll_key(key,
viewport_height)` entry point that only the four new variants use —
`App::handle_key` itself keeps its current signature so the other
~20 `InputKey` variants pay no plumbing cost for a feature that does
not concern them.

**6. Every other key on `Screen::Source` stays a no-op.** `Back` (Esc/
`q`) continues to return to the entry view. `d`/`R`/`o`/`s` and the
tree/right-pane keys do *not* leak through — the source screen is a
reading surface with one action ("go back") beyond scrolling, and
opening pane toggles here would re-open the "same key, different
behavior per screen" trap ADR 0020 exists to close.

## Alternatives

- **Source-screen scrolling only, defer entry-view half-page/top-bottom
  to a later ADR.** Considered because it is the smallest possible
  change; rejected because the user's own request explicitly extended
  to the diff pane, and both surfaces share the same four `InputKey`
  variants, same `translate_key` cases, and same viewport-height
  plumbing — splitting into two ADRs would duplicate the design
  work for no real reduction in blast radius.
- **Bind top/bottom to single-key `g`/`G` instead of `gg`/`G`.**
  Rejected on `Screen::Entry`: `g` alone is already the leading key of
  `gd`/`gr` (ADR 0022), and re-binding it to "top of pane" would
  either break the two-key sequences or force a per-screen split of
  what `g` means (the exact "same key, different behavior per screen"
  trap the last paragraph of decision 6 calls out). `gg` also happens
  to be what vim uses for this, so the transferability argument runs
  the same direction anyway.
- **Fold half-page steps into `App::handle_key` with a fixed constant
  step (e.g. 12 lines) so `App` does not need viewport height.**
  Rejected: on a 6-line pane that would jump nearly two full screens;
  on a 60-line pane it would barely register. The viewport-height
  plumbing is small and one-time; the height-agnostic constant would
  be wrong forever after.
- **Represent "scroll to bottom" as its own `Screen::Source` /
  `RightPane` state variant rather than a `usize::MAX` sentinel that
  the clamp folds down.** Rejected: every consumer that reads
  `scroll_top`/`right_pane_scroll` would then have to match on two
  variants forever after, for one boolean's worth of state that the
  existing `scroll_top.min(max_start)` clamp already collapses cleanly.
- **A separate scroll module with its own state machine for the
  source screen.** Rejected: this is exactly the "start simple, split
  when necessary" case where a parallel implementation of a pattern
  the codebase already models once (`right_pane_scroll`) creates a
  second surface for the two implementations to disagree, without
  producing anything new.
- **Auto-recenter on every scroll to keep the highlight visible.**
  Rejected: this actively fights the reviewer trying to read *away*
  from the highlight (the entire point of scrolling). Centering is
  correct exactly once — as the initial position on entering the
  screen — and becomes wrong as a per-frame rule the moment the
  reviewer starts scrolling.
- **Add a dedicated `App::handle_scroll_key(key, height)` entry point
  vs. extend `App::handle_key`'s signature to take an
  `Option<usize>`.** Rejected the latter: every `InputKey` variant
  other than the four new ones has no use for a viewport height, and
  every call site of `handle_key` (including every test) would grow
  an argument for a feature that does not touch it. A separately named
  entry point is the smaller, more targeted addition.

## Consequences

- The source screen now has scroll state that persists for the
  lifetime of one `Screen::Source` entry (not across `Back`/re-open —
  re-pressing `s` builds a fresh `Screen::Source` with a freshly
  centered `scroll_top`). This matches how the entry view's
  `right_pane_scroll` also resets on most transitions; a reviewer's
  expectation on re-opening a symbol is "show me the symbol again",
  not "resume where I left off".
- `crate::ui::draw`'s return signature grows to surface the actively
  scrolling pane's inner height alongside its existing clamped-scroll
  return. Mirrors the seam already used by
  `clamp_right_pane_scroll_after_draw`, so this is one more field on
  the same seam rather than a new one.
- `App` grows a small second entry point (`handle_scroll_key`) and the
  four new `InputKey` variants. The three other screen/focus branches
  of `handle_key` need no changes; `translate_key`'s `pending_prefix`
  resolver gains one line for `gg`.
- The pre-existing test asserting "the source screen scrolls via its
  own auto-centering window, not …" (`crate::ui`) is deliberately
  obsoleted by this ADR: its old assertion (source-screen `Up`/`Down`
  produce no scroll offset for `run_app` to fold back) becomes false
  by design. Rewritten to assert the new contract (source-screen
  `Up`/`Down` update `Screen::Source.scroll_top`) rather than deleted,
  so a future regression that silently reverts to no-op is caught.
- The four new keys and the `gg` resolution get their own entries in
  the `?` help overlay and the status-line hint set, so the addition
  is discoverable without reading this ADR — this is what ADR 0020's
  own "no in-app help" complaint was trying to prevent from
  reappearing.
- No backward-compatibility concern: the TUI has never shipped a
  release (ADR 0015/0016), so this is a strict addition to the
  interaction model. `Ctrl-d`/`Ctrl-u`/`gg`/`G` were previously
  unbound on both screens.
