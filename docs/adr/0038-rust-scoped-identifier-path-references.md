# 0038. Capture the path side of Rust `scoped_identifier` as a type reference

- Status: accepted
- Date: 2026-07-14

## Context

`RustSupport::REFERENCE_QUERY` (ADR 0003's tags-based, name-only
resolution) has no pattern for `scoped_identifier` at all — the node
tree-sitter produces for `Type::Thing` (enum-variant construction,
associated-item access, or a UFCS call's callee). A changed function whose
body only references another changed type through this path form (for
example `OutputFormat::Markdown`) produces no `referenced_names` entry for
`OutputFormat`, so `graph::build_graph`'s name matching never links the
two symbols — the dependent shows up as an unconnected root instead of an
edge to the type it depends on.

The existing `call_expression function: (identifier)` pattern deliberately
excludes `scoped_identifier` callees (`Type::method()`) because resolving
a method name to its defining type needs type information v1 doesn't have.
That exclusion is about the *name* (right-hand) side of the path — it says
nothing about the *path* (left-hand) side, which names a real type/enum
and is exactly the kind of reference `type_identifier` already captures
elsewhere.

## Decision

Add `(scoped_identifier path: (identifier) @reference.type)` to
`REFERENCE_QUERY`. This captures the left-hand identifier of any scoped
path — `OutputFormat` in `OutputFormat::Markdown`, `Type` in
`Type::method(1)`'s UFCS callee, `Self` in `Self::Default` — without
capturing the right-hand `name` field (method/variant/associated-item
name), so the UFCS-callee exclusion is unchanged: only the type-shaped
side is added.

Because a `scoped_identifier`'s `path` field can itself be a nested
`scoped_identifier` rather than a bare `identifier` (`a::b::Type::method`
parses as `path: (path: (path: identifier "a") name: "b") name: "Type")
name: "method"`), the query only matches when a path segment's own `path`
is a bare identifier. For a two-segment path (`Type::method`) this
captures `Type` as wanted; for a three-or-more-segment path
(`a::b::Type::method`), the type-shaped segment (`Type`) sits one level
too deep and is missed — only the outermost module segment (`a`) is
captured by the query, and `extract::is_noise_name`'s existing
single-character filter drops it anyway when it is that short. This is a
known, accepted gap in the syntax-only approach (module-qualified paths
are rare in practice compared to the direct `Type::variant`/
`Type::method` form this fixes) and is pinned by a test rather than
silently left unspecified.

## Alternatives

- **Match `scoped_identifier` unconditionally (both `path` and `name`
  fields)**: would also capture the UFCS method/variant name as a
  reference, re-introducing the exact ambiguity ADR 0003's exclusion
  comment rules out (an unresolvable bare method/variant name matched
  against unrelated same-named definitions). Rejected.
- **Recursively unwrap nested `scoped_identifier` paths to always reach
  the innermost type-shaped segment**: would fix the 3+-segment case but
  requires distinguishing "the last path segment before the final `name`"
  from "a module segment," which the grammar doesn't mark — segments look
  identical whether they are modules (`a`, `b`) or the type (`Type`).
  Deferred; not worth the complexity for a form uncommon in the codebases
  rinkaku targets.
- **Do nothing (leave `scoped_identifier` unhandled)**: keeps the current
  silent gap, where a large and common Rust idiom (returning/constructing
  another changed enum by its variant path) never produces a dependency
  edge. Rejected — this was the motivating bug.

## Consequences

- Enum-variant and associated-item path references (`Type::Variant`,
  `Type::CONST`, a UFCS call's `Type::method(...)`) now contribute their
  left-hand type name to `referenced_names`, so `graph::build_graph` can
  link a changed function to a changed type it only references this way.
  This is what restores cross-file/cross-directory edges the TUI's
  topological ordering depends on.
- `Self::Variant`/`Self::method(...)` also now capture the literal text
  `"Self"`, matching `type_identifier`'s pre-existing behavior for
  `-> Self` return types (already unfiltered before this change) — no new
  special-casing needed.
- Deeply nested module-qualified paths (`a::b::Type::method`) still don't
  resolve their type segment; if this proves common enough to matter,
  revisit with a recursive-unwrap approach then.
