# 0024. One-hop inbound context beyond the diff

- Status: proposed
- Date: 2026-07-13

## Context

The symbol graph (`rinkaku-core/src/graph.rs`, `collect_edges`) only
creates edges between *changed* symbols; references to unchanged code
are dropped at graph-build time. As a result, the TUI's `gd`/`gr` jump
navigation (ADR 0022) frequently has zero candidates on small or
loosely-coupled diffs, even when the changed symbol clearly has callers
or callees just outside the diff.

Dogfooding feedback: focusing the tree on the diff is the right default,
but reviewers still want minimal surrounding context — especially
*inbound* callers ("who calls this changed symbol, from code the diff
doesn't touch"). Outbound 1-hop context already exists as data:
`ResolvedSymbol` (`rinkaku-core/src/deps.rs`), produced via `--deps`,
carries a signature and a path for each dependency, but has no line
number and is currently unused by the TUI. Inbound 1-hop has no data
source today at all — surfacing it would require a repo-wide reference
scan (e.g. a tags query over unchanged files), which nothing in the
codebase currently performs.

## Decision

Surface 1-hop context beyond the diff as **read-only display** (e.g. in
the detail pane or the blast radius pane), prioritizing inbound callers
over outbound callees, since inbound is the harder gap (no data source)
and the more frequently requested one in dogfooding. `gd`/`gr` jump
semantics (ADR 0022) stay limited to symbols already in the change
graph — an out-of-diff symbol has no tree row to land on, and precise
navigation into unchanged code is the editor/LSP's job, not this tool's.

This ADR does not yet commit to a data model or a rendering location;
those are implementation decisions for a follow-up ADR or PR once the
inbound-scan approach is validated.

## Alternatives

- **A Mermaid structure diagram of the current change graph** (ADR
  0021): rejected as a standalone fix for this gap — it renders the
  same sparse, changed-symbols-only graph, so it does not add the
  missing out-of-diff context.
- **Full jump support into unchanged symbols**: deferred — it needs line
  numbers on `ResolvedSymbol`, pseudo tree nodes for symbols with no
  diff hunk, and a file-viewer-like surface to land on; heavy relative
  to the review-support goal for a first cut.
- **Status quo (no out-of-diff context at all)**: rejected — the
  "surprisingly can't jump / no idea who calls this" friction is a real,
  recurring dogfooding complaint, not a hypothetical one.

## Consequences

- Inbound context requires a repo-wide scan, which has a cost (time,
  and possibly false positives from name-only matching) not yet
  measured against real repositories.
- Outbound context likely requires extending `ResolvedSymbol` with line
  information (and possibly a module/module-path field) to be useful
  for display, not just for text.
- This ADR stays `proposed` until the scan approach and the exact
  surfaced data are validated against dogfooding; a follow-up ADR should
  record the concrete design once scope is agreed, and may supersede
  this one.
