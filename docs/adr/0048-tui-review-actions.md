# 0048. TUI review notes: location-anchored notes with explicit export sinks

- Status: Proposed
- Date: 2026-07-14

## Context

The TUI (ADR 0015/0016) is a read-only reviewing surface: a reviewer can
navigate the change, read signatures/diffs/source, and pivot through
dependencies, but has no way to act on what they find without leaving the
tool. Turning an observation into an action today means manually copying
text out of the terminal and switching to a browser (to leave a PR
comment) or to an AI coding agent (to ask for a fix) — the TUI's own
"never leaves the terminal" principle (ADR 0015's drill-down decision)
stops at reading.

This ADR proposes a v1 review-notes feature. Two earlier shapes of this
design were considered and rejected before settling on the one below
(both recorded in Alternatives): a single capture mechanism with an
implicit fallback destination, and two separate capture verbs split by
audience at compose time. Both put an audience decision (human reviewer
vs. AI agent) somewhere it doesn't belong — either hidden behind runtime
context or forced too early, before the reviewer necessarily knows who
should read a given note. The shape this ADR settles on instead keeps
note-taking audience-neutral and pushes the audience decision to an
explicit export step.

## Decision

**The primitive is a single, destination-neutral `Note { location,
body }`.** `location` is a GitHub-independent abstraction — a file path,
a new-side line range, and an optional symbol anchor — not a GitHub
review-comment shape; nothing about a `Note` says who will eventually
read it. **Composing a note is one verb** (e.g. `c`, opening a text-input
overlay over the currently selected symbol/diff line, mirroring the
existing overlay pattern used by the help overlay (ADR 0020) and the
jump-target popup (ADR 0022) rather than shelling out to `$EDITOR`,
consistent with ADR 0015's "the reviewer never leaves the terminal").
While reading, the reviewer does nothing but attach notes to locations;
no destination decision is made at this point, and none is needed — a
reviewer can flag a location before deciding whether the flag is
"a comment for the PR" or "an instruction for an agent," because at
compose time it is often both, or undecided.

**Export is a separate, explicit step that picks a sink**, run from a
notes list/summary view over the accumulated `Vec<Note>`. Three sinks:

- **Sink A — GitHub PR review, when `PrContext` is available.** `main.rs`
  already resolves everything a GitHub review needs during `--pr` mode:
  `PrInfo` (`rinkaku/src/github/pr_info.rs`) carries the PR number and
  head commit, and `PrArg::Url`/`git_remote_origin_url` +
  `parse_github_remote` (`rinkaku/src/github/remote.rs`) carry owner/
  repo — today these drive only base/head SHA resolution and are never
  threaded past `run_analysis`. This ADR adds a `PrContext { owner, repo,
  number, head_sha }`, assembled once in `main.rs` after `run_analysis`
  succeeds, and passed into `TuiSession::run` alongside the existing
  `report`/`diff_text`/`entry_path`/`repo_root` parameters
  (`Option<PrContext>`, `None` for every non-`--pr` input mode). Exporting
  to this sink lets the reviewer pick a **verdict** — approve, request
  changes, or comment-only — mirroring GitHub's own pending-review submit
  dialog; the export renders each `Note` into a human-addressed review
  comment body and posts the batch as one pending review (`gh api`: open
  → attach each comment → submit with the chosen verdict). GitHub's
  review API only accepts inline comments on lines that are part of the
  PR's diff; since the TUI's diff pane always renders the same diff
  `analyze_diff` consumed, every `Note`'s `location` is by construction a
  line the API will accept. **When `PrContext` is `None`** (stdin input,
  `--base` mode), this sink is simply absent from the export menu — not
  disabled-with-an-error, not silently rerouted; there is no implicit
  fallback to another sink (see Alternatives).
- **Sink B — clipboard, an AI-readable context packet.** Renders the
  full `Vec<Note>` plus each note's originating signature/hunk/fan-in
  context (the same data `crate::detail::build_detail` already computes)
  into a Markdown packet addressed to an AI agent, and writes it via an
  OSC 52 escape sequence (see Alternatives for why OSC 52 over a
  clipboard crate or shelling out to `pbcopy`/`xclip`). Available
  regardless of `PrContext` — this sink never touches GitHub.
- **Sink C — interactive agent session (scope fixed here, implementation
  deferred to a follow-up PR).** The reviewer selects a configured shell
  command (e.g. `claude`, `codex`, or any other CLI the user configures —
  no vendor-specific integration lives in `rinkaku-core`/`rinkaku-tui`,
  only a configurable command string); the TUI suspends itself, runs that
  command interactively attached to the same terminal (passing sink B's
  packet via stdin or a temp file, TBD at implementation time), and
  resumes the TUI event loop when the child process exits. This differs
  from ADR 0015's "never leaves the terminal" in degree, not in kind: the
  terminal itself is never left, but the TUI's own alternate screen is
  torn down for the duration of the child process — the same suspend/
  resume shape `TuiSession::init`/`run`'s alternate-screen lifecycle
  already has to support cleanly for the terminal-restoration guarantees
  `session.rs` documents (`Drop`, panic hook). This ADR fixes sink C's
  shape so sinks A/B are not designed into a corner, but defers its
  implementation — see Consequences.

**Destination-specific formatting is each sink's own responsibility, not
the note's.** A pure render function per sink turns the audience-neutral
`Vec<Note>` (plus whatever ambient context that sink needs) into the
wording appropriate for its reader — sink A's render wraps a note as a
human-addressed review comment, sink B/C's render wraps the same note as
an AI-addressed instruction. `Note` itself carries no
"phrased-for-a-human" vs. "phrased-for-an-agent" distinction; the same
notes list feeds every sink's render.

**Phased scope: v1 ships note composition + sink A + sink B. Sink C ships
once its own PR lands**, keeping this PR's surface to note capture, PR
review submission (with verdict selection), and clipboard export.

**Scope boundary: reading, replying to, resolving, or displaying
existing PR review threads is a non-goal, not a deferred goal.** This
ADR's designs (a thin `Note` primitive, export sinks rather than a full
review inbox) were guided by one question applied throughout: **does the
map make this feature better?** rinkaku's differentiated asset is the
map — signatures, fan-in, dependency graph, blast radius — computed from
`Report`; a feature the map has nothing to say about is exactly the kind
of feature GitHub's own web UI already does richer, with less
implementation cost, than a terminal reimplementation ever could. Writing
notes benefits from the map (a note is anchored to a location the map
already understands — a symbol, a hunk, a blast-radius neighborhood);
reading an existing comment thread's back-and-forth does not — a thread
is conversational content the map has no signature/fan-in/dependency
signal to add to. This is why this ADR keeps the TUI's own new surface
to composing and exporting notes, and does not grow it into a general
review inbox mirroring github.com. If a future ADR revisits the read
path, the same test caps its scope: only a **marker** — "this location
already has N review comments," anchored on the tree/diff pane the same
way this ADR's own `NoteMarkers` (see the rendering-boundary decision
below) marks a reviewer's own notes — is in bounds, since a marker is
map-anchored positional information; opening, threading, or replying to
a comment inline is out of bounds regardless of phase, since none of
that benefits from being inside the map at all and belongs in the
browser tab already showing it.

**Module boundary: review notes live in a new, self-contained module
(`rinkaku-tui/src/review/`), coupled to the rest of the TUI through one
narrow input type and one narrow output type — not through shared state.**
This applies this project's `CLAUDE.md` "ports as traits, defined on the
consumer side" / "composition root in `main.rs`" principles one layer
down (between `rinkaku-tui` modules, not just at the crate/OS boundary),
the same discipline ADR 0016 already applies to `LanguageSupport`/
`Resolver`:

- **Own state.** The module owns everything this feature needs — the
  accumulated `Vec<Note>` and the compose-overlay's in-progress text —
  behind its own type (e.g. `ReviewState`). `App` holds exactly one field
  of that type; `App`'s tree/nav/diff-view state and `review`'s state
  never reach into each other.
- **Input boundary: a selection snapshot, not `Report`/`App` access.** A
  new `SelectionSnapshot { path, new_line_range, symbol_id, signature }`
  (or equivalent), derived by a small function in the existing `app`/
  `detail` layer from whatever the cursor currently points at, is the
  sole channel by which `review` learns what the reviewer is annotating.
  `review` never holds a `&Report` or reaches into `App`'s tree/nav
  fields directly.
- **Output boundary: values and a thin port, not side effects.** `review`
  itself returns only plain data — the `Vec<Note>`, and each sink's
  rendered `String` — never calling `gh`, touching the clipboard, or
  spawning a process itself. Every side effect (the `gh api` calls, the
  OSC 52 write, launching the configured agent command) sits behind a
  small port (a Rust trait, 1–3 methods, defined where consumed) that
  `rinkaku` (the binary crate) implements and wires up by hand at the
  composition root (`main.rs`), mirroring how `LanguageSupport`/
  `Resolver` are wired today. `rinkaku-tui` depends on the port's trait
  definition at most, never on a concrete `gh`/clipboard/process
  implementation.

**Rendering boundary: the compose overlay and notes/export menu are new
`ui::overlay`-style draw functions; existing-pane note indicators are
v1 scope, driven by a marker set precomputed once per state change, never
inside `ui::draw`.** Grounded in reading the actual draw path
(`rinkaku-tui/src/ui/mod.rs`, `rinkaku-tui/src/ui/overlay.rs`,
`rinkaku-tui/src/row_view.rs`, `rinkaku-tui/src/tree/mod.rs`,
`rinkaku-tui/src/lib.rs`'s `run_app`), three existing patterns this
feature must fit rather than invent a fourth:

- **Overlays are drawn by dedicated functions in `crate::ui::overlay`,
  fed plain data, called from `crate::ui::draw`.** `draw_help_overlay`
  and `draw_jump_popup` (`ui/overlay.rs`) both follow the same shape:
  `Clear` the popup's `Rect`, then render `Paragraph`/`Line`s built from
  a value already sitting on `App` (`app.help_scroll()`,
  `app.jump_popup(): Option<&JumpPopup>` — `JumpPopup` is a plain,
  `PartialEq`-comparable struct field on `App`, not a separate module).
  `crate::ui::draw` composites each one on top of the base screen as a
  final step, gated on `App`'s own flag/`Option` (`app.help_open()`,
  `app.jump_popup()`), exactly mirroring `Screen::Entry`/`Screen::Source`
  compositing order. The compose overlay and the notes-list/export-menu
  overlay follow this precedent exactly: new functions
  `draw_note_compose_overlay`/`draw_export_menu` in `ui/overlay.rs` (or a
  sibling `ui/review_overlay.rs` if the module grows past ADR 0028's
  threshold), fed the `review` module's own plain state
  (`review.compose_draft(): Option<&str>`, `review.notes(): &[Note]`,
  an export-menu equivalent of `JumpPopup`), gated in `crate::ui::draw`
  on an `App`-exposed accessor the same way `help_open()`/`jump_popup()`
  already are — **not** implemented as methods that reach into
  `rinkaku-tui`'s tree/nav/diff state, matching the "own state, narrow
  input" module-boundary decision above.
- **Existing-pane note indicators (a tree-row badge, a diff-line marker
  for "this location has a note") are in v1 scope**, following the same
  precomputed-badge precedent `Badges`/`push_badge_spans` already
  establish: `Badges` (`tree/mod.rs`) is computed once, bottom-up, when
  `build_tree` runs (`App::new`) and stored directly on `TreeNode` — a
  tree row's draw call (`row_view::entry_row_line`) only ever reads
  `row.node.badges`, a field access, never a query against session
  state. This feature's indicator follows the same shape but cannot be
  baked into `TreeNode` at tree-build time (notes accrue *after* the
  tree is built, as the reviewer works) — the read-only precedent that
  fits is `crate::lib::run_app`'s cache-on-change pattern already used
  for `blast_radius_selection`/`diff_pane_content`: a derived,
  read-only `NoteMarkers` set (e.g. `HashSet<String>` of `TreeNode`/
  `Note::location` paths, or path→count) is recomputed **once, in
  `run_app`, only when `review`'s note set actually changes** (on
  compose-confirm, delete, or export — a `should_recompute_note_markers`
  gate mirroring `should_recompute_blast_radius_selection`/
  `should_recompute_diff_pane_content`'s own contracts), then passed
  into `ui::draw`/`row_view::entry_row_line`/`ui::diff_pane` as a plain
  `&NoteMarkers` argument for that pane's badge-push functions
  (`push_badge_spans`-equivalent) to read. `row_view`/`ui::diff_pane`
  read `NoteMarkers`, they never read `review`'s own `Vec<Note>` or
  compose-overlay state directly — the same "plain derived data in, no
  live session state" shape `diff_pane_header_lines` already takes
  `badges: &Badges` rather than the tree or session it came from.
- **`ui::draw` itself runs on every ~100ms idle poll, not only on a key
  press** (`run_app`'s own doc comments on `diff_hunks`/
  `diff_highlights`/`blast_radius_selection`/`diff_pane_content`, and the
  regression each one's comment cites — the blast-radius view and the
  diff-pane content were each, at different points, mistakenly
  recomputed inside the draw path and had to be pulled back out to a
  once-per-handled-key cache in `run_app`). `NoteMarkers`' derivation
  (a walk over `Vec<Note>`, unbounded in the number of notes a long
  review session accrues) must not be called from inside
  `crate::ui::draw`, `row_view::entry_row_line`, or `ui::diff_pane` for
  the identical reason — it is computed in `run_app` alongside
  `blast_radius_selection`/`diff_pane_content` and only on the same
  kind of change-gated schedule, never per frame.

- **Touch points on existing code are limited to four:**
  - **(a) `InputKey` gains the note/export variants** (compose, confirm/
    cancel, open export menu, choose sink, choose verdict for sink A),
    and `App::handle_key` routes them to the `review` field — the same
    "translate a key, dispatch to the owning state" shape every existing
    `InputKey` variant already follows (`app/input_key.rs`,
    `app/handle_key.rs`).
  - **(b) A selection-snapshot derivation function**, built from the same
    cursor/tree-node data `crate::detail::build_detail` already reads, so
    `review` never needs its own copy of tree-walking logic.
  - **(c) `main.rs` wires up `PrContext` and the port implementation(s)**
    at the composition root, and passes both into `TuiSession::run`.
  - **(d) Rendering**: two new `ui::overlay`-style draw functions (compose
    overlay, notes/export menu), one new derived-and-cached
    `NoteMarkers` value threaded through `run_app` alongside
    `blast_radius_selection`/`diff_pane_content`, and one new read-only
    parameter each on `row_view::entry_row_line` and the Diff pane's
    line-rendering functions to consult it.

  No other existing module changes. **`rinkaku-core` is untouched** —
  this feature is entirely a `rinkaku-tui`/`rinkaku` concern, consistent
  with this project's practice of keeping the pure core stable across
  TUI-only features (ADR 0043's Consequences note the same for its own
  TUI-only change).

## Alternatives

- **Build a full review inbox (fetch, display, thread, reply to, resolve
  existing PR comments) as part of this ADR's scope**, rather than
  declaring it a non-goal. Rejected on the map test the Scope boundary
  decision states: a comment thread is conversational content, not
  something signatures/fan-in/dependency-graph/blast-radius has anything
  to contribute to, so building it would spend this feature's complexity
  budget re-implementing what GitHub's own web UI already does better,
  for no map-derived advantage. Keeping the non-goal explicit (rather
  than leaving it merely unaddressed) is what prevents the TUI from
  drifting into a general review client one incremental feature at a
  time.
- **Implicit fallback: if `PrContext` is absent, silently route notes to
  the clipboard sink instead of GitHub.** Rejected: this is an
  audience-mismatch bug, not a convenience — the reviewer never chose an
  audience, so a fallback the tool picks on their behalf can hand
  human-addressed phrasing to an AI agent (or vice versa) without the
  reviewer's intent ever entering the decision. Making export an explicit
  step with an always-visible sink menu (sink A merely absent when
  `PrContext` is `None`) means the reviewer always makes the destination
  choice themselves.
- **Split compose into two verbs at write time** (a `comment` key
  addressed to a human, a separate `agent handoff` key addressed to an
  AI, each its own state machine) — an earlier shape of this ADR.
  Rejected as over-splitting: the audience distinction is a property of
  *where a note ends up*, not of the note itself, and forcing the
  reviewer to pick an audience while still reading requires them to
  predict, mid-review, which destination a given observation will
  eventually go to — often before they know yet whether they'll submit a
  PR review, ask an agent to fix it, or both. Keeping the note itself
  destination-neutral and deferring the audience choice to export
  (Decision above) removes that upfront guess and lets one note feed
  either or both sinks' renders without being written twice.
- **Route notes through `$EDITOR` instead of an in-TUI overlay.**
  Rejected: reintroduces exactly the terminal-leaving ADR 0015 rejected
  for source viewing, and an `$EDITOR` round-trip means suspending the
  alternate screen for every single note rather than only for sink C's
  already-interactive session.
- **Post each note individually as it's written, instead of batching
  into one review at export time.** Rejected: GitHub's per-comment
  endpoint creates a separate notification per call and cannot be
  submitted/discarded as a unit; the pending-review API (start review →
  add comments → submit with a verdict) matches how a human reviews on
  github.com and lets the reviewer discard the whole batch if they
  change their mind before submitting.
- **Have `rinkaku-tui` call `gh` directly for sink A**, instead of
  returning notes as values for `main.rs` to post through a port.
  Rejected: violates the IO-at-the-boundary split every prior TUI ADR
  maintains (ADR 0016, `session.rs`'s doc comment) — `rinkaku-tui` would
  gain a process-spawning dependency and a test surface that can only be
  exercised against a real `gh` binary, the opposite of the pure-view-
  model discipline the crate is built around.
- **Fold `review`'s state into `App` directly** (a `Vec<Note>` field and
  compose-overlay state sitting alongside `Screen`/`RightPane`/`Focus`),
  instead of a separate `review` module behind a `SelectionSnapshot`
  boundary. Rejected: `App` already composes four stage-A view-models
  (`tree`, `nav`, `order`, `detail`, this module's own doc comment), each
  read by `App::handle_key`'s existing match arms; adding review state
  directly would make it just another set of fields a change to tree/nav/
  diff-view code has to reason about not breaking, and vice versa. A
  narrow `SelectionSnapshot` input keeps the coupling to one plain-data
  type instead of shared mutable state.
- **Clipboard via a crate dependency (`arboard`) instead of OSC 52.**
  Considered for sink B; OSC 52 (write the clipboard contents as an
  escape sequence to the terminal, which the terminal emulator itself
  intercepts and loads into the system clipboard) is the recommended
  approach: it adds no new dependency, works transparently over SSH
  (where `arboard`, which talks to a local clipboard API/X11/Wayland
  directly, cannot reach a remote session's clipboard at all), and
  degrades safely — an unsupporting terminal simply ignores the escape
  sequence rather than erroring. The tradeoff is real: not every terminal
  emulator implements OSC 52 (support is inconsistent, and some disable
  it by default for security reasons), so the TUI must treat the write as
  best-effort and give the reviewer a visible fallback (the packet text
  itself, so it can be selected/copied by hand) rather than assuming
  success silently. Shelling out to `pbcopy`/`xclip`/`wl-copy` was also
  considered and rejected: it requires probing for whichever tool the
  host has installed, is platform-specific in a way OSC 52 is not, and
  still fails over SSH the same way `arboard` does.
- **Derive `NoteMarkers` inside `crate::ui::draw`/`row_view` at draw
  time**, reading `review`'s `Vec<Note>` directly on every frame, instead
  of precomputing it once in `run_app` on a change gate. Rejected on the
  same grounds `run_app`'s own doc comments already record twice over
  (`blast_radius_selection`, `diff_pane_content`): `ui::draw` runs on
  every ~100ms idle poll tick, not only on a key press, so a per-frame
  walk over an unbounded `Vec<Note>` would reintroduce the identical
  per-frame recompute bug this codebase has already had to fix twice —
  this time for a value that grows with session length rather than PR
  size, making the eventual slowdown less visible until a long review
  session hits it.
- **Omit existing-pane note indicators entirely from v1** (a reviewer
  would only see their notes from the dedicated notes-list/export menu,
  never as a badge/marker on the tree or diff pane they were reading).
  Considered as the simpler scope cut, but rejected in favor of including
  it: the tree pane's whole design principle (ADR 0043, ADR 0028) is
  surfacing risk/attention signals as badges exactly where the reviewer
  is already looking, and a note is exactly that kind of signal — "I
  already commented here" is information the reviewer needs at the row,
  not only in a separate list they have to remember to check. Since the
  precomputed-marker-set shape above reuses the existing badge/cache
  machinery rather than inventing new state-management, the marginal
  design cost of including it in v1 is small enough not to defer.
- **Bake note markers directly onto `TreeNode`/`Badges` at tree-build
  time**, mirroring how `Badges` itself is computed once in `build_tree`.
  Rejected: `Badges` is safe to bake in because it is derived entirely
  from the immutable `Report` the tree is built from once, at `App::new`,
  and never changes afterward. Notes are added *during* the session,
  after the tree already exists — baking them in would require rebuilding
  (or mutating) the tree itself every time a note is composed or deleted,
  a far more invasive change than a small side-table `run_app` already
  knows how to maintain and pass through, mirroring
  `blast_radius_selection`/`diff_pane_content`'s own precedent for
  exactly this kind of "changes with session state, not with `Report`"
  data.
- **Ship sink C in this same PR instead of deferring it.** Rejected:
  suspending the alternate screen for an interactive child process is a
  materially different terminal-lifecycle concern from anything
  `TuiSession` has done before (every existing mode transition stays
  inside the same alternate-screen session) and deserves its own focused
  review; fixing its shape here without implementing it keeps sinks A/B
  from being designed in a way that would need to change once sink C is
  built.

## Consequences

- `App` gains exactly one new field (`review: ReviewState` or
  equivalent); every exhaustive `match` over `InputKey`/`Screen` gains
  the new review-related arms, the same category of churn ADR 0043's
  `TestGroup` variant already caused.
- `TuiSession::run` gains a new `Option<PrContext>` parameter; `main.rs`
  gains the small amount of glue needed to assemble `PrContext` from
  `PrInfo` + `PrArg`/`git_remote_origin_url` after `run_analysis`
  succeeds — no change to `run_analysis` itself, since none of these
  values are new, only newly threaded further.
- Posting a review is the first network-writing action the TUI can
  trigger (every prior TUI action is read-only against local data); the
  `gh api` calls implementing it live in `rinkaku` alongside
  `fetch_pr_info`'s existing `gh` invocation, behind the port `review`
  itself never touches directly.
- Clipboard export is best-effort: a terminal without OSC 52 support
  silently does not populate the system clipboard, so the TUI must
  surface the packet text itself as a visible fallback rather than
  assuming the write succeeded.
- Sink C's shape (suspend TUI, run configured interactive command,
  resume) is fixed by this ADR but not implemented; a follow-up PR
  implements it against this shape rather than re-litigating the design.
- `run_app` gains one more cache-on-change value (`NoteMarkers`)
  alongside `blast_radius_selection`/`diff_pane_content`, and one more
  `should_recompute_*` gate function following their existing contract;
  `row_view::entry_row_line` and the Diff pane's line-rendering functions
  each gain one new read-only parameter to consult it. `ui::overlay`
  gains two new draw functions (or a sibling `ui/review_overlay.rs`
  module, per ADR 0028's file-size threshold) following
  `draw_help_overlay`/`draw_jump_popup`'s existing shape.
- `rinkaku-tui`'s `review` module is testable in isolation from the rest
  of the crate: its unit tests construct a `SelectionSnapshot` directly
  rather than a full `Tree`/`Nav`/`Report`, and its sink-render functions
  are tested the same way `render_digest` (ADR 0036) is — plain data in,
  `String`/`Note` values out.
- **Reading existing PR review threads (fetching, displaying, replying
  to, or resolving comments already on GitHub — posted by other
  reviewers, or by a prior run of this same feature) is a non-goal by
  the Scope boundary decision above, not merely deferred.** A future ADR
  may still add a positional **marker** ("this location already has N
  review comments") to the tree/diff pane, since a marker is map-anchored
  the same way `NoteMarkers` is — but full thread display/reply/resolve
  is out of bounds regardless of phase, on the same "does the map make
  this better" test, and is left to GitHub's own web UI.
