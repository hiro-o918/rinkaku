# 0022. Renaming the TUI's pivot pane to "blast radius"

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0019 added a right-pane mode, internally and in the UI called
"pivot", that re-roots the dependency tree at a selected directory or
file and shows what it reaches outward into. ADR 0020 gave it a key
(`p`) and a glossary entry in the `?` help overlay. Dogfooding this
interaction model (the same round that produced ADR 0020) surfaced two
concrete complaints about the pane itself, independent of the
interaction-model work ADR 0020 already fixed:

- "pivot がなにかわからん" — "pivot" names the *mechanism* (re-rooting
  the graph at a path) rather than the *question a reviewer is asking*
  when they press the key. A reviewer scanning a PR is not thinking "I
  want to re-root the dependency graph"; they are thinking "if I touch
  this, what else breaks" — the exact framing ADR 0021 already uses in
  its own Context section ("blast-radius help matters most").
- "cycle の意味がわからん" — the `⚠️ <label> — dependency cycle, see
  above` marker names the graph-theory concept (a closing back-edge)
  without saying what it means for reading the tree: that the reviewer
  already saw this node earlier in the same expansion and does not need
  to expand it again.

The underlying feature (ADR 0019's re-rooting) is not in question —
dogfooding confirms it is useful, only mis-named. This ADR is scoped to
naming and presentation in the TUI, not to the graph algorithm, the CLI
flag, or `rinkaku-core`'s public API.

## Decision

**1. Rename the TUI-facing surface from "pivot" to "blast radius".**
This covers:

- The key: `p`/`P` is retired; `R` (mnemonic: **R**adius) toggles the
  pane. Since the TUI has never shipped a release (ADR 0020's own
  "no backward-compatibility concern" applies again here), this is a
  plain rename, not a deprecation with a transition period.
- The `RightPane` variant: `RightPane::Pivot` becomes
  `RightPane::BlastRadius`. Likewise `PivotSelection` ->
  `BlastRadiusSelection`, `InputKey::TogglePivot` ->
  `InputKey::ToggleBlastRadius`, `pivot_return_pane` ->
  `blast_radius_return_pane`, `App::selected_pivot_view` ->
  `App::selected_blast_radius_view`, the `crate::pivot` module ->
  `crate::blast_radius` (`PivotView`/`PivotLine` ->
  `BlastRadiusView`/`BlastRadiusLine`), and `ui::draw_pivot_pane` ->
  `ui::draw_blast_radius_pane`.
- Pane chrome: the block title changes from `" Pivot "` to a header
  that states the question the pane answers, not the mechanism —
  `"Blast radius of <path>"` — so the pane is self-explanatory without
  opening `?` first. The placeholder/empty-state text is updated to
  match ("select a directory or file row to see its blast radius" /
  "nothing under `<path>` is reachable").
- Help overlay: the `KeyBindingGroup`/`GlossaryEntry` entries move from
  "pivot" wording to "blast radius" wording (decision 3 below).

**2. `rinkaku-core`'s graph API and the CLI's `--entry` flag are
unchanged.** `graph::pivot_graph`, `graph::pivot_roots`, `--entry`,
`apply_entry_pivot`, and `entry_pivot_empty_note` in `rinkaku/src/
main.rs` keep their existing names. Rationale:

- `--entry` is a released-to-users-mentally CLI surface name
  independent of the TUI's internal pane naming — the flag re-roots the
  *Markdown output*, which has no "pane" to name, and "entry point" is
  already the vocabulary ADR 0008 established for the whole-report root
  concept it extends. Renaming it would trade one unfamiliar term for
  a different unfamiliar term with no dogfooding signal asking for it.
- `graph::pivot_graph`/`pivot_roots` are internal `rinkaku-core` API,
  consumed by both the CLI and the TUI; renaming them would touch a
  crate this ADR has no dogfooding complaint about and couple an
  internal-API rename to a UI-wording rename for no functional reason.
  The TUI's `crate::blast_radius` module continues to call
  `rinkaku_core::graph::pivot_graph` — a pure naming mismatch between
  the layers is acceptable here the same way `rinkaku-tui`'s
  `detail.rs` already uses "pivot" for a different, unrelated concept
  (callers/callees framing, ADR 0015) without confusion, because that
  usage is a doc-comment word, not UI-facing text.
- This keeps the rename's blast radius (no pun escaped) to exactly the
  TUI crate's user-facing surface plus its own internal names — the
  layer boundary this project already draws between "what
  `rinkaku-core` exposes" and "what the TUI calls it" absorbs the
  rename without rippling into the CLI or core.

**3. Cycle marker gets a plain-language rewrite, not just a rename.**
The line `⚠️ <label> — dependency cycle, see above` becomes `! <label>
— already shown above (cycle)`: leads with the actionable fact ("you've
seen this, stop expanding") and demotes "cycle" to a parenthetical for
readers who want the graph-theory term. The marker itself is a plain
`!`, not `⚠️` — dynamic verification (`tmux capture-pane` against a real
build, per this project's mandatory review step) caught `⚠️` (U+26A0 +
a U+FE0F variation selector) desyncing `unicode-width`'s column count
from the terminal's actual double-column rendering of the pair, which
left a stray character on screen in the blast-radius pane; this was the
exact risk flagged in this ADR's own Alternatives before implementation,
confirmed rather than assumed. `rinkaku-core::render`'s Markdown output
keeps `⚠️` unchanged — it is plain text there, never fed through a
terminal-cell width calculation, so the bug is specific to the TUI's
`ratatui`-rendered pane. The help overlay's glossary entry for "cycle"
is rewritten the same way: from "A closing back-edge in the dependency
graph — two or more directories depend on each other" to "A dependency
loop: two or more symbols depend on each other, so the tree stops and
points back to where it first appeared." The glossary's "pivot" entry
is replaced by a "blast radius" entry: "The dependency tree rooted at a
selected directory or file, showing what would be affected if it
changed."

**4. ADR 0019 and ADR 0020 are not edited.** They are historical
records of the decisions made at the time, including the "pivot" name
this ADR retires — rewriting their prose would misrepresent what was
actually decided when. This ADR supersedes their *naming* only; their
architectural content (the re-rooting algorithm in ADR 0019, the focus
model and pane-mode state machine in ADR 0020) stands unchanged. Future
readers of ADR 0019/0020 should cross-reference this ADR for the
current UI vocabulary.

## Alternatives

- **Keep "pivot" as the internal name, only change the pane's display
  title**: cheaper (one string change), but leaves the key hint
  (`p: pivot`), the help overlay's keymap group entry, and every error
  message using a term dogfooding already flagged as confusing —
  rejected because it fixes the symptom the pane shows on first glance
  and leaves the same word everywhere else the reviewer looks next
  (status line, help overlay).
- **Rename `rinkaku-core::graph::pivot_graph`/`pivot_roots` and
  `--entry` too, for full-stack consistency**: rejected per decision 2
  — no dogfooding complaint targets the CLI or core naming, and
  bundling an uncomplained-about rename into this change would enlarge
  the diff and the review surface without a matching benefit.
- **Superseding ADR 0019 outright (marking it superseded, not just
  cross-referenced)**: ADR supersession in this project (see ADR
  0021's own precedent of adding new decisions alongside old ones
  rather than rewriting them) is reserved for when a *decision* is
  reversed, not when a *name* changes; the re-rooting behavior ADR
  0019 decided is still exactly what ships. Rejected in favor of a
  cross-reference.
- **Keep the emoji-based `⚠️` cycle marker as-is, only reword the
  text**: considered, but since the wording change already touches
  every call site that constructs the line, this ADR takes the
  opportunity to confirm (rather than assume) the marker renders
  correctly in the terminal during implementation, per CLAUDE.md's
  "Dynamic verification" review requirement. It does not: `tmux
  capture-pane` against a real build showed a stray character next to
  the marker (decision 3's own explanation). Rejected in favor of a
  plain `!`, confirmed rather than assumed.

## Consequences

- The TUI's public vocabulary (key hints, pane titles, help overlay,
  README) now names the reviewer's *question* ("what's the blast
  radius of this change") rather than the *mechanism* ("re-root the
  graph at this path") — directly answering the dogfooding complaint
  that motivated this ADR.
- A one-time rename cost across `rinkaku-tui`: every `Pivot`-prefixed
  type, field, and function in the crate moves to `BlastRadius`-
  prefixed, touching `app.rs`, `lib.rs`, `ui.rs`, `help.rs`, and the
  renamed `pivot.rs` -> `blast_radius.rs`, plus their tests. This is a
  mechanical, crate-internal rename (decision 2 keeps it from crossing
  into `rinkaku-core` or the CLI), so the risk is test-update volume,
  not design risk.
- `rinkaku-core`'s `pivot_graph`/`pivot_roots` and the CLI's `--entry`
  flag now use different vocabulary than the TUI pane built on top of
  them. This is an accepted, explicit split (decision 2), not an
  oversight — a future reader diffing `crate::blast_radius`'s calls
  into `rinkaku_core::graph::pivot_graph` should not read the name
  mismatch as a bug.
- Future TUI panes should default to naming the reviewer's question
  first and reserve mechanism-derived names (like the original
  "pivot") for internal/doc-comment use only — this ADR is the
  concrete precedent to point to next time a pane's UI name is chosen.
- `rinkaku-tui` should treat multi-codepoint emoji (base glyph +
  variation selector, e.g. `⚠️` = U+26A0 + U+FE0F) as suspect for any
  string that passes through `crate::ui::wrap_lines`'s column
  accounting, not just visually — `unicode-width`'s per-`char` model
  does not necessarily agree with a real terminal's rendered column
  width for such pairs. This ADR's own dynamic-verification step is the
  concrete instance; future additions of emoji markers to TUI-rendered
  (not Markdown) text should verify in a real terminal before landing,
  not just in a `TestBackend` snapshot (which does not model this
  particular class of desync).
