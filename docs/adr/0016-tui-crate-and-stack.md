# 0016. TUI crate and stack

- Status: accepted
- Date: 2026-07-12

## Context

ADR 0015 accepted splitting rinkaku's audiences: Markdown and JSON stay
frozen, machine-readable output formats, and humans get a dedicated
interactive terminal UI instead of ever-denser static text. That ADR
settled the *what* (an entry view listing the directory tree of changed
files with badges, a detail pane showing a symbol's signature diff plus
its used-by/callers/callees, and drill-down to highlighted source) but
left the *how* — stack, crate placement, testability — for this ADR.

Two upcoming ADRs feed data this UI needs: ADR 0013 adds a `hotspots`
field to `Report` (fan-in aggregation), and ADR 0014 adds symbol change
classification (added / signature-changed / body-only, plus a removed
list and previous signatures for diffs). Both extend the same `Report`
struct (`rinkaku-core/src/render.rs`) that Markdown rendering already
consumes — the TUI is a second consumer of that struct, not a new data
model.

The workspace is currently two crates (`rinkaku-core`, `rinkaku`; ADR
0001), split so `rinkaku-core` stays free of CLI-only dependencies. The
same reasoning applies to a TUI, an even heavier dependency (raw
terminal handling, an immediate-mode widget tree) than anything
currently in `rinkaku-core`.

## Decision

**1. Framework: `ratatui` with the `crossterm` backend.** It is the
de-facto standard Rust TUI stack, actively maintained, and its
immediate-mode rendering model (redraw the whole frame from current
state every tick) fits a read-only viewer: there is no mutable widget
tree to keep in sync with `Report`, only a view-model rebuilt from it.

**2. Crate structure: a new workspace member `rinkaku-tui`.** A library
crate depending on `rinkaku-core` directly (same process, not a
subprocess or IPC boundary) for `Report` and its nested types
(`FileReport`, `SymbolGraph`, and the ADR 0013/0014 additions once
merged). The existing `rinkaku` binary crate depends on `rinkaku-tui` and
invokes it from the composition root (`main.rs`), the same place that
currently picks Markdown vs. JSON via `OutputFormat`.

`rinkaku-core` stays pure: the TUI is another adapter beside
`render_markdown`/`render` (JSON), not a change to core's dependency
graph. All IO the TUI needs beyond `Report` itself — opening a changed
file to show highlighted source in the drill-down view — is adapter-side
file reads in `rinkaku-tui`, mirroring how `main.rs` already owns every
other IO boundary (git invocation, file reads for diff input).

Whether the TUI is reached via a `tui` subcommand or a `--tui` flag on
the default command is an open point for the implementation PR, leaning
toward a flag: the input flow (diff in, from stdin/`--base`/`--pr`) is
unchanged, only the output stage differs, and `clap`'s `Subcommand` is
reserved for commands with a genuinely different input contract (see
`main.rs`'s `Commands` enum).

**3. View-model separation for testability.** Every layout-independent
computation — directory-tree building from `Report`'s file paths, badge
aggregation (changed-symbol counts, ADR 0013 hotspot markers, ADR 0014
added/removed/signature-changed counts), the topological display order
(decision 4 below), and selection/navigation state transitions — lives
in plain functions and structs with no `ratatui` types in their
signatures. These are unit-tested with `rstest` +
`pretty_assertions::assert_eq!` comparing whole values, per the repo's
existing test conventions (see `rinkaku-core/src/render.rs`'s test
module). Rendering itself is covered separately with `ratatui`'s
`TestBackend` buffer snapshots, kept few and coarse — enough to catch a
broken layout, not to pin every pixel.

**4. Planned view features, recorded as scope (implementation order
free):**

- **Topological directory ordering by default.** The entry view's
  directory tree is ordered by SCC-condensing the change graph down to
  directories, showing outermost/least-depended-on directories first and
  foundational (most-depended-on) ones last — reusing the condensation
  approach `rinkaku-core`'s root-finding already performs at the symbol
  level (see `graph::find_roots`'s SCC handling). A directory-level cycle
  shares a rank with the rest of its cycle and is surfaced as a design
  warning, consistent with a symbol-level cycle already rendering as a
  warning line in Markdown (ADR 0008). An A-Z toggle remains available.
- **Whole-repository mode.** Diffing against git's empty tree makes
  every symbol "changed", turning rinkaku into a whole-codebase outline.
  This becomes the intended default when none of `--pr`, `--base`, or
  piped stdin input is given (TTY check at the CLI boundary, not inside
  `rinkaku-core`) — Markdown stays impractical at that scale, so the TUI
  is the intended surface for it. Changing the input-mode default is a
  distinct, later decision with its own small ADR when implemented; this
  ADR only records the TUI as its target surface.
- **Entry-path perspective.** Keep the whole-graph analysis but restrict
  roots to symbols under a user-chosen path prefix — "the outline as
  seen from `api/`" — as a pure roots filter at the graph layer in
  `rinkaku-core` (no new IO, no new data shape). In the TUI this is a
  pivot action on a directory node in the entry view, and it is also the
  antidote to root explosion in whole-repository mode.

## Alternatives

- **`cursive` (retained-mode alternative):** smaller ecosystem and less
  momentum than `ratatui`; a retained widget tree also fights the
  pure-view-model split above, since retained widgets encourage storing
  view state on the widgets rather than deriving it from `Report` every
  frame.
- **Raw `crossterm`/`termion` with no TUI framework:** reimplements
  layout and widget composition from scratch for no benefit over
  `ratatui`, which already solves that problem.
- **A separate binary/crate distributed independently from `rinkaku`:**
  already rejected in ADR 0015 — splitting distribution/versioning at
  this size adds packaging overhead without a corresponding benefit.
- **Driving the TUI off JSON output via a subprocess** (shelling out to
  `rinkaku --format json` and parsing stdout) instead of linking
  `rinkaku-core` directly: adds a serialization boundary and a
  version-skew risk inside what is meant to be a single tool. Direct type
  reuse is simpler, and the JSON contract remains available unchanged
  for actual external tools that need a subprocess boundary.

## Consequences

- A new, fairly large dependency subtree (`ratatui`, `crossterm`, and
  transitives) enters the Cargo workspace, but only in the new
  `rinkaku-tui` crate — `rinkaku-core`'s dependency set (listed in the
  workspace's `[workspace.dependencies]`) is unchanged, preserving ADR
  0001's goal of keeping the core embeddable without extra weight.
- TUI implementation work can proceed as soon as `rinkaku-core` exposes
  the data it needs; it depends on ADR 0013/0014 landing in `Report` but
  requires no further core changes beyond consuming those fields (and,
  later, the roots-filter for the entry-path perspective).
- `ratatui` `TestBackend` buffer-snapshot tests are coarser than the
  fully-qualified whole-value asserts the rest of the repo favors for
  logic tests; accepted because rendering carries little logic once the
  view-model is correct, and the view-model layer keeps the repo's usual
  `rstest` + `pretty_assertions` discipline.
- The `tui` subcommand vs. `--tui` flag question, and the whole-
  repository default-input-mode change, are left open and settled (the
  latter via its own ADR) at implementation time rather than blocking
  this decision.

## Addendum: `crossterm`'s `use-dev-tty` feature (2026-07-13)

Bare `rinkaku` (stdin attached to a terminal) worked from the start, but
`gh pr diff 123 | rinkaku` — the README's own primary usage example, and
ADR 0017's stdin input mode — could not open the TUI at all: it consumes
stdin to read the diff, and `crossterm`'s default event source (the `mio`
backend) tries to poll stdin itself for keyboard input rather than
falling back to the controlling terminal, so as soon as stdin is a pipe
rather than a TTY it fails outright with "Failed to initialize input
reader" (verified with a minimal `crossterm::event::read()` reproduction,
independent of any `rinkaku-tui` code). `--tui` explicitly requested
alongside a piped diff hit the same failure.

Fix: `rinkaku-tui/Cargo.toml` now depends on `crossterm` directly (in
addition to the `ratatui::crossterm` re-export used everywhere else in
the crate) solely to enable its `use-dev-tty` feature. Cargo's feature
unification applies that feature to the single shared `crossterm`
instance ratatui also depends on — there is no second copy of the crate.
With `use-dev-tty`, `crossterm`'s Unix event source opens `/dev/tty`
directly for keyboard input whenever stdin is not a TTY, independent of
what stdin is being used for, which is exactly the piped-diff case.
Verified interactively (`tmux`): a piped diff plus a raw `crossterm`
probe blocks correctly on `/dev/tty` and reports real key presses once
this feature is enabled, versus erroring immediately without it.

`use-dev-tty` is Unix-only (it is not defined for the Windows backend);
accepted because CI (`.github/workflows/*.yaml`) only runs `ubuntu-latest`
and `macos-latest` today, so there is no Windows target to regress. It
pulls in `filedescriptor` and `rustix/process` as additional transitive
dependencies — both small, no further action needed.

Alternative considered: closing and reopening stdin from `/dev/tty` by
hand in `rinkaku-tui::run` before starting the event loop. Rejected as
strictly more code for the same outcome `use-dev-tty` already implements
and tests upstream in `crossterm` itself.
