# 0003. Tags-based dependency resolution first, LSP-backed resolvers later

- Status: accepted
- Date: 2026-07-11

## Context

Showing only the signatures of changed symbols is often not enough context
for a reviewer or an LLM — a changed function's signature may only make
sense alongside the types or helper functions it depends on. rinkaku
therefore needs to expand each changed symbol to include its immediate
dependencies. Resolution can be done at different levels of precision,
from purely syntactic (no type information) to fully type-aware (requires
a working project build).

## Decision

v1 resolves dependencies one hop out using tree-sitter tags queries: for
each changed definition, find identifiers it references and match them
against definitions in the same tags index. This is approximate (no type
resolution, no cross-crate/package resolution beyond what tags queries
capture) but requires no project build or setup. A `Resolver` trait is
defined as the extension point so that LSP-backed resolvers (pyright,
gopls, rust-analyzer, ...), run as external processes, can be plugged in
later as an opt-in, higher-precision alternative for v2+.

## Alternatives

- **LSP-backed resolution from v1**: more precise (real "go to
  definition" / "find references"), but requires the target project to
  build and its language server to index it first — too slow and too
  fragile as the default for arbitrary CI checkouts. Deferred to the
  `Resolver` trait as a v2+ pluggable option instead of the default.
- **No dependency expansion (signatures only)**: simplest, but defeats
  the purpose for reviewers who need to see a changed type's shape, not
  just that a function signature changed.
- **Full transitive closure instead of 1-hop**: more complete, but risks
  pulling in large portions of the codebase for widely-used types,
  working against the tool's goal of condensing diffs.

## Consequences

- Zero-setup, fast dependency expansion that works the same way the core
  extraction engine does (ADR 0002) — no build step required.
- Resolution can be wrong or incomplete in cases syntax alone can't
  disambiguate (e.g., overloaded names, dynamic dispatch, shadowing).
- The `Resolver` trait boundary must be designed now, even though only
  the tags-based implementation ships in v1, so LSP resolvers can be
  added without reshaping the core pipeline.
- 1-hop is a deliberate depth limit; revisit if user feedback shows
  reviewers need transitive expansion (with size safeguards).
