# 0015. Split audiences: TUI for humans, Markdown/JSON for machines

- Status: accepted
- Date: 2026-07-12

## Context

ADR 0012 condensed the change-graph rendering and removed noise from
Markdown output. Dogfooding since then shows the remaining human
complaints are presentational, not noise:

- Important rows — high fan-in symbols, contract changes — do not
  visually stand out in a flat Markdown list. A reviewer must read
  every line at equal weight to find the ones that matter.
- Full file paths repeated on every row force the reader to
  mentally re-parse the same prefixes over and over, when what they
  actually want is the directory *shape* of the change: nesting depth
  approximates architectural layering, and that shape is exactly what
  a reviewer uses to judge blast radius.
- Reviewers want to understand a change visually — scan, fold,
  pivot — not read a sequence of path strings top to bottom.

Static Markdown cannot express visual hierarchy, emphasis, folding, or
on-demand pivoting between views. ADR 0012 already pushed static
condensing about as far as string-level formatting goes; there is no
further condensing move left that does not start encoding UI concerns
(color, collapse state, navigation) into text. ADR 0012's Alternatives
section deferred an interactive TUI specifically because it "does not
fix the noise itself" — that objection targeted noise, which 0012 has
since resolved. It does not apply to the presentational problems above,
so that deferral is now resolved in favor of building the TUI; this
does not reopen or supersede the rest of ADR 0012.

The maintainer's workflow is terminal-centric; a web UI is explicitly
out of scope regardless of its technical merits.

Architecturally this fits without disruption: `rinkaku-core` is pure
and the `Report`/graph model already serializes to JSON (ADR 0008,
ADR 0010). An interactive front-end is simply another adapter reading
the same `Report`, alongside the existing Markdown renderer — it adds
a consumer, not a change to the domain model.

## Decision

Split the two audiences at the format level going forward:

1. **Markdown and JSON are optimized for machine consumers** — LLMs
   and CI tooling. They stay stable, parseable, and rich in
   precomputed aggregates (fan-in counts, change classification; see
   prerequisites below). No further Markdown work is aimed at human
   visual comfort: no mermaid diagrams, no directory-grouping
   sections, no color/emphasis markup in Markdown.
2. **Human consumption moves to an interactive terminal UI**, shipped
   as part of rinkaku, built on the same `Report` data. This ADR fixes
   only the direction, at the concept level; framework and
   implementation details are a separate track (see Consequences):
   - The entry view is the **directory tree of changed files**, not
     the call-graph tree. Nesting depth conveys architecture; each
     directory/file row carries aggregate badges (changed-symbol
     count, a contract-change marker, fan-in).
   - Selecting a symbol opens a **detail pane**: its signature (an
     old→new diff when the contract changed, per the change
     classification prerequisite below), its fan-in as a "used by"
     list, and a pivot to callers/callees for call-graph reading
     order (ADR 0008's tree, reachable on demand rather than as the
     spine).
   - Drill-down bottoms out in **viewing the actual file source**
     with the relevant lines highlighted — the reviewer never leaves
     the terminal to open an editor.
   - The call-graph tree (reading order) and the directory tree
     (architecture) are orthogonal hierarchies. The TUI keeps the
     directory tree as the spine and reaches call relations by
     pivoting, rather than merging both into one combined tree.

## Alternatives

- **Keep improving Markdown for humans** (mermaid module diagrams,
  directory-grouping sections, path-prefix trimming): string-level
  tweaks cannot deliver real visual hierarchy or interaction, and each
  addition bloats the same format that machine consumers depend on
  staying stable and parseable.
- **Web UI**: would satisfy the visual/interactive requirement, but
  requires leaving the terminal and serving/opening a browser, which
  is not the primary workflow. Rejected on that basis, not on
  technical merit.
- **One hybrid output serving both audiences**: this tension — humans
  wanting hierarchy and folding, machines wanting stable flat
  structure — is the root cause of the current complaints. Splitting
  the audiences dissolves the tension instead of continuing to
  compromise both sides.
- **Separate standalone viewer tool consuming rinkaku's JSON**: keeps
  rinkaku itself lean, but splits distribution and versioning across
  two projects for no user benefit at this stage, since the TUI is
  still small. Revisit if the TUI grows large enough to justify its
  own release cycle.

## Consequences

- Markdown format can now freeze toward LLM/CI ergonomics; future
  human-facing feature requests route to the TUI backlog rather than
  new Markdown sections or flags.
- The TUI is a significant new surface — framework choice (e.g.
  ratatui), event handling, and testing strategy are deliberately out
  of scope here. A follow-up ADR will fix the tech stack once
  implementation starts.
- Two data prerequisites must land before the TUI's badges and detail
  pane are meaningful: fan-in aggregation (ADR 0013) and change
  classification with old→new signature diffing (ADR 0014). Until
  then, this ADR fixes direction only; implementation waits on that
  data.
- This resolves the TUI deferral recorded in ADR 0012's Alternatives
  section (the "does not fix noise" objection no longer applies to
  the presentational problems being solved here); it does not mark
  ADR 0012 itself superseded.
