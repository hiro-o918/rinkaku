# 0008. Entry-point tree rendering for changed symbols

- Status: accepted
- Date: 2026-07-12

## Context

The current Markdown output lists changed symbols flat, in git file order,
each with its 1-hop dependencies (ADR 0003). Two things make this hard for
humans to read. First, there is no obvious place to start reading — file
order carries no meaning. Second, `resolve_dependencies` deliberately
excludes symbols that are themselves part of the diff, so the relationships
*between* changed symbols — the most interesting edges in a review — are
invisible, and the reader has to reconstruct the call graph mentally.

The data to fix this already exists: each `ExtractedSymbol` keeps its raw
`referenced_names`, and the tags index can match those names against the
other changed symbols.

## Decision

Build a directed graph over the changed symbols (edges from
`referenced_names` matched against the changed-symbol set) and render
Markdown as trees rooted at auto-detected entry points: changed symbols
that no other changed symbol references. Unchanged 1-hop dependencies
remain leaf annotations as today. A symbol reachable from multiple roots
is rendered in full once and referenced by name afterwards. Cycles among
changed symbols are broken at the back edge and rendered with an explicit
warning marker, since a dependency cycle usually signals a design smell
the reviewer should see. The JSON output gains the same edge information;
this is a breaking change to both output formats.

## Alternatives

- **User-specified entry point (`--entry <symbol>`)**: requires the user
  to already know the change's structure, which is exactly what the tool
  should surface. Graph roots give the same starting points for free. May
  still be added later as a *filter* on top of this rendering.
- **Keep the flat list and append a separate call-graph section**: avoids
  a breaking change, but duplicates every symbol and still forces readers
  to join two sections mentally.
- **Split human (tree) and LLM (flat) formats behind a flag**: structure
  helps LLM consumers too, and maintaining two Markdown renderers doubles
  the surface for every future change. Revisit only if LLM consumption
  measurably degrades.

## Consequences

- Markdown reads top-down in call-hierarchy order: entry points first,
  callees nested beneath, giving reviewers a natural reading order.
- Dependency cycles among changed code become visible as warnings instead
  of being silently flattened away.
- Name-based edges inherit the imprecision of tags resolution (ADR 0003):
  a false match now distorts tree shape, not just a dependency list, so
  wrong edges are more visible — acceptable while the `Resolver` trait
  allows a precise LSP-backed upgrade later.
- Both output formats break; downstream consumers must adapt. Acceptable
  pre-1.0.
