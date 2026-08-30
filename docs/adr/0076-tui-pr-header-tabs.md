# 0076. TUI PR header tabs: show the reviewed PR(s) at the top of the frame

- Status: proposed
- Date: 2026-08-30

## Context

ADR 0075 gave `--pr` mode a status-line prefix (`PR #43 2/3  |  `) so a
stacked-PR session shows which layer is on screen, but that prefix sits
at the bottom, competes for the same 80-column budget as the key hints,
and gives no view of the *other* layers — a reviewer cannot tell how far
through the stack they are, which layers are already analysed, or which
one failed, without pressing `gt`/`gT` to find out. Single-PR `--pr`
sessions have no on-screen identity at all beyond the terminal's own
scrollback.

## Decision

### D1. A one-row header, `--pr` mode only

`ui::draw` reserves one row at the very top of the frame
(`Layout::vertical`) whenever the session has a `PrContext` — stack or
single-PR — and omits it entirely otherwise. stdin/`--base` sessions
keep today's layout byte-for-byte; no existing non-`--pr` rendering test
changes.

### D2. Stack mode renders a label, the trunk, and one tab per layer; single-PR mode renders a plain title line

Stack mode: `stack 2/2  main ▸ #2063 auth ▸ #2055 api`, bottom-to-top,
current layer bold with the crate's accent color
(`crate::ui::style::pane_border_style`'s `Color::Cyan`, the same "this one
is active" signal the focused-pane border already uses), a
`Pending`/`InProgress` layer dimmed with a trailing `…`, a `Failed` layer
dimmed with a trailing `!`. The leading `stack k/n` label (1-based cursor
position over layer count) is always shown regardless of width — every
other element budgets around it rather than trying to drop it. Every
layer's tab stays visible for as long as it can: full titles (capped at
24 display columns even when the terminal is wider, so a stack of several
layers doesn't let the current tab's title swallow all spare width), then
titles shrunk to an equal per-tab column budget (a fixed floor of 8
columns, split evenly across tabs rather than proportionally to title
length — simpler to reason about, and every tab ends up equally readable
rather than the longest title being squeezed hardest), then numbers only.
The stack's base branch name (the trunk, e.g. `main`) is shown ahead of
the tabs, joined by ` ▸ `, whenever it fits alongside every layer at the
current title width; it is dropped before any tab is ever dropped or
title-shrinking abandoned for numbers-only, so a reviewer sees "this
layer is on top of that layer, which is on top of trunk" for as long as
there is room, and loses the trunk's own name first when there isn't.
Only once even a numbers-only strip of every layer (without the trunk)
doesn't fit does the strip scroll, dropping tabs off one end to keep the
current layer in view.

Single-PR mode: `PR #249  <title>` — the title truncated to whatever
width remains, no other segment.

### D3. Segment computation is a pure function; the cache is read live at draw time

The text/segment layout (what to show, in what order, with what
truncation) is a pure function of `(width, current index, per-layer
label + slot status)`, taking no `ratatui` types beyond `usize` in and
returning plain `Segment { text, kind }` values out. `crate::ui::pr_header`
turns `Segment`s into styled spans; the pure function itself is
unit-tested with `rstest`.

Per-layer slot status (`Pending`/`InProgress`/`Ready`/`Failed`) is read
directly from `PrAnalysisCache` on every draw (`cache.get(i)`, one
`Mutex` lock per layer per frame) rather than pushed into `App` through
the event loop — the cache already exists as the single source of truth
for background-analysis progress (ADR 0075 D3), and a frame redraws on
every ~100ms idle poll regardless of a key press, so reading it directly
keeps the header always current without adding a second update path the
event loop would have to remember to drive.

`run_app` takes `Option<Arc<PrAnalysisCache>>` (`None` for single-PR/
non-`--pr` sessions) and stores it on `App` unchanged, alongside the
`PrContext` the current layer is running with — a small addition to
`App`'s otherwise `Report`-derived state (`App::with_stack`'s own
precedent for other ADR 0075 session-scoped fields).

### D4. `PrContext` gains a `title`

Both the single-PR path and stack mode need a title to render — a stack
layer already carries one (`StackEntry`/`StackPr`), a single `--pr`
session did not fetch one before this change. `PrContext` gains
`title: String`; `rinkaku/src/github/pr_info.rs`'s `PrInfo` and its
`gh pr view --json` field list gain `title` to fill it for the
single-PR path, and `rinkaku/src/stack_run.rs` fills it from the
already-available `StackPr::title`. `pr_url`/sink A posting are
unaffected — neither reads `title`.

### D5. The status-line stack prefix is removed; the Tree-focus hint's `enter: open` returns; `/: search` is the new stack-mode drop

With the header now showing stack position, the status line's `PR #43
2/3  |  ` prefix (ADR 0075 D5) is redundant and is removed
(`stack_prefix`/its tests deleted). `enter: open` is restored to the
Tree-focus hint unconditionally — the least discoverable loss ADR 0075
D5 accepted no longer needs to be paid, since removing the prefix frees
more budget than `enter: open` costs.

Stack mode adds a `gt/gT: PR` hint so the header's own navigation keys
are discoverable from the status line the same way every other binding
is, but `enter: open` + `gt/gT: PR` together no longer fit the
80-column budget (#196) alongside every other hint. Stack mode drops
`/: search` instead: search is a Tree-pane feature orthogonal to which
PR is on screen, already fully documented in the `?` overlay, and the
header's own tab strip already gives a stack session more on-screen
orientation than a non-stack session has — the same "least
discoverable loss" logic ADR 0075 D5 used, applied to the entry this
ADR's own new hint pushes out.

## Alternatives

- **Push cache state into `App` via the event loop.** Would make the
  header's data flow symmetric with everything else `App` owns, but
  requires the event loop to poll the cache every idle tick (mirroring
  the version-check channel poll) purely to keep a value in sync that
  the draw call could read directly with no extra plumbing. Rejected:
  more code for a cache that is already safe to read from the render
  path.
- **One paragraph string for the whole header, no per-segment styling.**
  Simpler, but loses the current/pending/failed visual distinction that
  is the point of a tab strip. Rejected.
- **Keep the status-line prefix alongside the header.** Redundant
  information in two places competing for the same narrow budget.
  Rejected (D5).

## Consequences

- `ui::draw`'s frame layout gains a conditional row; every screen
  (Entry, Source) sits one row lower when a `PrContext` is present.
- `App` gains two more session-scoped fields (`Option<PrContext>`,
  `Option<Arc<PrAnalysisCache>>`), following `stack`'s existing
  precedent.
- `run_app`/`TuiSession::run`/`run_stack` thread one more optional
  parameter through; `TuiSession::run`'s single-PR call site passes
  `None`.
- `PrContext` gains a field; every struct-literal construction (`main.rs`,
  `stack_run.rs`, and every test fixture) adds `title`.
- Non-`--pr` sessions (stdin, `--base`) are visually and behaviorally
  unchanged.

## Amendment history of amended ADRs

- **Amends ADR 0075 D5**: the status-line stack prefix this decision
  introduced is removed once the header (this ADR) carries the same
  information; the `enter: open` hint it dropped is restored.
