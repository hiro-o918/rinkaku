# 0037. Render removed symbols in `--format mermaid`

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0021 defined `--format mermaid` as a head-side call/dependency
graph: one node per changed/reachable symbol, grouped into a
`subgraph` per file, styled by contract-impact classification
(`added`/`changed`/`fan-in`). It renders from `report.graph`, which is
built only from head-side symbols (`graph::collect_nodes` over
`report.files`) — a symbol that existed on the base side and was
deleted entirely never enters `report.graph` at all, so it never had a
node to render.

ADR 0014 already tracks these: `Report.removed: Vec<RemovedSymbol>`
(name, kind, path, base-side signature) is populated by
`extract::classify_symbols` for exactly this case, and
`render_markdown` has shown a "## Removed symbols" section from it
since that ADR. `render_mermaid`/`render_mermaid_file_level`
(`rinkaku-core/src/render/mermaid.rs`) have never read `report.removed`
— a reviewer looking at the mermaid graph alone cannot tell that a PR
deleted a function, only that it doesn't mention one. For a graph
whose whole pitch (ADR 0021) is "what talks to what at a glance," a
silent gap where a deleted symbol used to be is a real blind spot: the
graph shows what's new and what changed, but not what's gone.

This ADR amends ADR 0021 to close that gap: `render_mermaid` and its
file-level fallback both grow a `removed` node class covering the same
data `render_markdown`'s "## Removed symbols" section already
surfaces.

## Decision

Render each `RemovedSymbol` as a node in the same graph, not a second
graph.

- **One merged graph, not separate as-is/to-be diagrams.** An
  alternative considered was two side-by-side flowcharts (base graph,
  head graph) so a reader could visually diff them. Rejected: this
  format's primary consumer is a PR comment read by both a human
  reviewer and an LLM-review pass (`docs/experiments/0001-...`); two
  full graphs cost roughly double the rendered/token size for a
  capability (reconstructing base-side edges) `report.graph` doesn't
  even carry today (see Alternatives) — for the price of "twice as
  much to read," a merged graph already answers "what got added,
  changed, or removed" in one pass.
- **Removed nodes render as isolated nodes, not edges.** A removed
  symbol has no entry in `report.graph.edges` — `report.graph` is
  built exclusively from head-side symbols (see Context), so there is
  no base-side edge data to draw from. Reconstructing what a removed
  symbol used to call/be called by would require building a *second*
  graph over base-side symbols, which is a materially bigger change
  (a new extraction pass over base content, not just a render-layer
  read of data that already exists) and is out of scope here — flagged
  in Consequences as a possible future ADR if a concrete need for it
  shows up.
- **Placement: inside the removed symbol's file `subgraph`, same as
  every other node.** `RemovedSymbol.path` already identifies which
  file it belonged to; grouping it with the file's other (surviving)
  nodes keeps the "one subgraph per file" grouping meaningful — a
  reader scanning a file's subgraph sees everything that changed in
  that file, added/changed/removed alike, rather than removed symbols
  floating in an unrelated location.
- **Styling: a new `removed` classDef, dashed border.** Distinguishing
  color from `added`/`changed`/`fan-in` (all solid-border, warm/cool
  fills) via a muted gray fill (`#e2e8f0`) with a **dashed** stroke
  (`stroke-dasharray: 5 5`) — the dash reinforces "this no longer
  exists" the same way ADR 0021 already uses a dashed *edge* (`-.->`)
  for a cycle, giving the format one consistent visual vocabulary for
  "not a normal solid connection/node." `removed` never collides with
  `fan-in`'s precedence rule (ADR 0021): a removed symbol is never a
  graph node with a classification wired through `SymbolLookup`, it's
  a separate `RemovedSymbol` list with no `graph.nodes` entry, so the
  two class assignments are computed independently and cannot both
  apply to the same node id.
- **Node label: name only**, same as every other node — ADR 0021's
  "coarse and readable, not a second Definitions" rule applies
  unchanged. A considered alternative, embedding the removed symbol's
  base-side signature text in the node label, is rejected for the same
  reason ADR 0021 rejected signatures in labels generally: the
  Markdown "## Removed symbols" section already shows the signature,
  and cramming it into a graph label reintroduces the noise ADR 0021
  explicitly designed away from.
- **Node budget includes removed nodes.** `MERMAID_NODE_BUDGET`'s
  check becomes `graph.nodes.len() + removed.len() > MERMAID_NODE_BUDGET`
  — a PR that deletes 25 symbols and adds 10 is exactly the kind of
  large change the budget exists to catch (ADR 0021), and excluding
  removed nodes from the count would let such a diff dodge the
  file-level fallback and render a hairball anyway.
- **File-level fallback also represents removed-only files — but only
  when the file itself survives the diff.** Before this change,
  `render_mermaid_file_level` only visited paths reachable through
  `report.graph.nodes` — a file that still exists but had every one of
  its symbols individually deleted (so it contributes zero head-side
  nodes, e.g. every function in `allgone.rs` removed by hunks while the
  file itself is kept, even if only as a comment) would not appear as a
  node at all. This amendment folds `report.removed`'s paths into the
  same file/path enumeration so that file still gets a node (classed
  `removed` if it has no other changed/added node).

  **This does *not* cover a whole-file deletion** (`ChangeKind::Deleted`
  — the file itself is gone from the head side, not just emptied of
  symbols). `rinkaku-core/src/pipeline.rs`'s `analyze_diff` classifies
  `ChangeKind::Deleted` files as `SkippedFile { reason:
  SkipReason::Deleted }` and `break`s out of that file's processing
  *before* `classify_against_base` (ADR 0014's removed-symbol
  classification) ever runs — so a wholesale file deletion never adds
  anything to `report.removed` at all, and is therefore invisible to
  both `render_mermaid_file_level` and `render_markdown`'s "## Removed
  symbols" section. This is a **pre-existing gap dating back to ADR
  0014**, not a regression introduced here or by ADR 0036: it was
  confirmed by dynamic verification of this PR (deleting a file
  wholesale produces `"skipped": [{"reason": "deleted"}]` and an empty
  `"removed"`, versus a partial deletion — file survives, one symbol
  inside it removed — which populates `report.removed` correctly
  today). Tracked as a follow-up:
  [#115](https://github.com/hiro-o918/rinkaku/issues/115).
- **Determinism**: removed nodes are appended in `report.removed`'s
  existing order (already diff-derived, see `RemovedSymbol`'s
  producer, `extract::classify_symbols`) after a file's head-side
  nodes, so within a subgraph the ordering is "surviving symbols, then
  removed symbols," stable and easy to reason about.

## Alternatives

- **Two flowcharts (as-is graph + to-be graph), reader diffs them
  visually.** Rejected: doubles rendered size/token cost in a format
  whose primary audience already includes an LLM-review pass reading
  it as context, for a benefit (seeing exact before/after edges) this
  ADR's merged-graph approach mostly delivers anyway via node styling.
- **Reconstruct base-side edges for removed nodes** (so a removed
  node's former calls/callers show as edges too). Rejected as out of
  scope: `report.graph` has no base-side edge data today: building it
  would mean parsing and graph-building over base content, a
  pipeline-level change, not a render-layer one. Revisit only if a
  concrete review workflow demonstrates it's needed enough to justify
  that pipeline work.
- **Embed the removed symbol's signature in its node label.** Rejected
  for the same reason ADR 0021 keeps every other label to a bare name:
  the Markdown output already carries the signature, and the graph's
  job is "what talks to what," not a second copy of "Definitions".
- **Exclude removed symbols from the node budget calculation.** Would
  let a bulk-deletion diff dodge the file-level fallback and render an
  oversized graph anyway, defeating the budget's purpose (ADR 0021).

## Consequences

- `render_mermaid`/`render_mermaid_file_level` now read
  `report.removed` in addition to `report.graph`/`report.files`; every
  other node/edge/class-assignment behavior for added/changed/fan-in
  nodes is unchanged, and a report with an empty `removed` list
  produces byte-identical output to before this ADR.
- The `removed` classDef joins `added`/`changed`/`fan-in` as a fourth
  shared constant in `MERMAID_CLASS_DEFS`, always emitted regardless of
  whether any removed node is present, same as the existing three (ADR
  0021 already emits all `classDef` lines unconditionally).
- Removed nodes are visually inert (no edges), which is an accurate
  representation of what data is available, not a limitation hidden
  from the reader — the leading file-level `%%` comment and this ADR's
  Decision make that explicit for anyone reading rinkaku's own
  documentation, and a reviewer wanting base-side call relationships
  still has the Markdown "## Removed symbols" section's signature text
  as a manual cross-reference.
- Reconstructing base-side edges (see Alternatives) remains open for a
  future ADR if a concrete workflow need for it emerges; this ADR does
  not foreclose it, only scopes it out for now.
- **Whole-file deletions are skipped before classification and stay
  invisible to this format** (pre-existing, ADR 0014 —
  [#115](https://github.com/hiro-o918/rinkaku/issues/115)): only a file
  that *survives* the diff but loses some or all of its individual
  symbols appears via `report.removed`. A PR that deletes a file
  outright shows nothing for that file in `--format mermaid` — not
  even a `removed`-classed node — which this ADR does not fix, only
  documents and tracks.
