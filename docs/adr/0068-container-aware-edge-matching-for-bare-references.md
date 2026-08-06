# 0068. Container-aware graph edge matching for bare references

- Status: accepted
- Date: 2026-08-06
- Relates to: [ADR 0003](0003-name-based-dependency-resolution.md)
  (name-only resolution, unchanged by this decision), [ADR
  0064](0064-scoped-and-method-call-references.md) (introduced the
  `@reference.method` capture this ADR extends), [ADR
  0012](0012-condense-change-graph-for-human-readability.md) (interface/
  trait method-spec references, whose captures this ADR reclassifies)

## Context

`graph::collect_edges` links a changed symbol to any other changed
symbol whose name matches an entry in its `referenced_names` (ADR
0003), with no further disambiguation. This is too coarse when the
referenced name is a *bare* call or type reference — one with no
receiver, e.g. `Foo()` in Python, Go, or TypeScript.

Bare references cannot syntactically name a symbol nested inside a
class/interface/trait: a method call on an instance is a member-access
expression (`obj.method()`), a distinct grammar shape that
`reference_query` deliberately does not capture as a bare identifier
for these three languages (see the doc comments on
`language::python::REFERENCE_QUERY`, `language::go::REFERENCE_QUERY`,
`language::typescript::REFERENCE_QUERY` — Rust is the one grammar that
already captures method calls, under the distinct `@reference.method`
capture added by ADR 0064). A bare reference therefore only ever
denotes a top-level definition or a same-container sibling, never an
arbitrarily-chosen member of an unrelated class.

`collect_edges` does not use this distinction: it matches purely by
name, so a bare `Foo()` call can wrongly link to `def Foo()` nested
inside an unrelated `class Baz`, reachable only via `baz_instance.Foo()`
in real code. Reproduction: a diff adds `class Baz: def Foo(self): ...`
in `a.py` while `b.py::use_foo()` bare-calls a top-level `Foo()` already
defined in `a.py` — `collect_edges` links `use_foo` to `Baz.Foo` instead
of (or in addition to) the correct top-level `Foo`.

The capture-kind information needed to fix this already exists at the
query level — `@reference.call`/`@reference.type` (bare) vs.
`@reference.method` (Rust's receiver-based calls) — but is discarded
during flattening: `extract::references::collect_referenced_names`
reads every capture whose name starts with `reference.` into one
`BTreeSet<String>`, and `ExtractedSymbol::referenced_names` stores the
merged result.

Two capture sites also mislabel this distinction today. Go's
`interface_type (method_elem name: ...)` and TypeScript's
`interface_declaration ... method_signature name: ...` (ADR 0012) both
capture a method-spec name — a name that, like Rust's
`@reference.method`, is meant to link to a container member (a receiver
method / class method), not a top-level symbol — but are captured under
`@reference.call` for want of a second capture prefix at the time (see
`language::go::REFERENCE_QUERY`'s doc comment, "captured under
reference.call ... since a method spec name plays the same role here as
a called function name"). Under name-only matching that mislabeling was
harmless; under container-aware matching it is not, since it decides
which matching rule applies.

## Decision

Preserve the capture-kind distinction end to end and use it in edge
matching:

1. **Reclassify container-referring captures as `@reference.method`.**
   Go's interface `method_elem` capture and TypeScript's interface
   `method_signature` capture change from `@reference.call` to
   `@reference.method` (Rust's two `trait_item`-scoped captures already
   use `@reference.method` after this ADR; before it, they used
   `@reference.call` — see the amendment note in Consequences). Python
   and HCL are unaffected: neither has a reference capture that names a
   contained symbol.
2. **Split `referenced_names` into two sets on `ExtractedSymbol`**:
   the existing `referenced_names` (bare `@reference.call`/
   `@reference.type` captures, plus the existing non-query code walks —
   macro bodies, module-scoped calls, HCL traversals) and a new
   `referenced_method_names` (`@reference.method` captures). Both stay
   `#[serde(skip)]`, matching `referenced_names`' existing "intermediate
   pipeline artifact" status.
3. **`graph::collect_edges` matches each set by a different rule**:
   a `referenced_names` entry may only edge to a changed symbol whose
   `container` is `None` (top-level) or equal to the referencing
   symbol's own `container` (a bare call from inside one container to a
   sibling member of that same container — the one case a bare
   reference can legitimately denote a contained symbol). A
   `referenced_method_names` entry keeps the prior unrestricted,
   any-container matching, since a receiver-based or method-spec
   reference is unambiguous about which shape of definition it can
   mean. Self-reference exclusion is unchanged.
4. **`pipeline::collect_referenced_names`** (the pre-pass that seeds
   `TagsResolver::new`'s prefilter) unions both sets — the prefilter's
   job is "don't index names nothing in the diff could reference,"
   which both sets equally qualify for.
5. **`deps::resolve_dependencies`'s candidate enumeration is
   unchanged** — it still loops `symbol.referenced_names` only (bare
   set), so a bare reference's dependency candidates are not yet
   container-filtered the way graph edges now are. This is a known gap,
   tracked as a follow-up issue rather than folded into this ADR: fixing
   it changes `dependencies`/`omitted_matches` output, a materially
   different blast radius than the graph-only fix here, and deserves
   its own measurement pass the way ADR 0064 did.

## Alternatives

- **A name-collision heuristic in `collect_edges`** (e.g. only apply
  the container filter when two changed symbols share both name and
  differing containers): rejected — it cannot distinguish a legitimate
  bare same-container reference from the bug case without the same
  capture-kind information this decision already threads through, so it
  would just be this decision implemented less directly, with an
  implicit rather than explicit rule.
- **A per-language `bool` on `LanguageSupport`** ("this language's bare
  references can reach containers"): rejected — the property is not
  language-wide, it is per-capture (Rust has both a genuinely bare
  `@reference.call` and a receiver-based `@reference.method` in the
  same grammar), so a language-level flag is the wrong granularity.
- **Resolve via LSP/type information instead**: the precise fix, and
  already the long-term plan for the `Resolver` trait (ADR 0003); out of
  scope for a syntactic, name-only-model fix.

## Consequences

- Fixes the false-edge case in `graph::SymbolGraph` (and therefore
  entry-point trees, fan-in, and test coverage, all built on top of it)
  for Python, Go, and TypeScript bare references. JSON output shape is
  unchanged — `referenced_names`/`referenced_method_names` are both
  `#[serde(skip)]`, so no new field appears in `Report`.
- Legitimate edges are preserved: top-level-to-top-level bare calls,
  same-container bare calls (e.g. one method of a class bare-calling a
  module-level helper the class also happens to define — rare, but the
  same-container allowance covers it), and every method/method-spec
  reference (Rust `x.foo()`, Rust trait methods, Go interface specs, TS
  interface specs) keep matching any container exactly as before.
- Go's and TypeScript's interface/method-spec captures move from
  `@reference.call` to `@reference.method`, which routes them through
  `extract::is_ubiquitous_method_name`'s stoplist (ADR 0064) — a
  filter that previously applied only to Rust's `field_expression`
  captures. An interface/method-spec name that happens to collide with
  a stoplist entry (`get`, `len`, ...) now goes unmatched to its
  implementing method, same tradeoff ADR 0064 already accepted for
  Rust's receiver calls, extended here rather than carved out with a
  narrower filter scope — narrowing the filter to "only Rust
  `field_expression` captures" would need to thread capture-site
  provenance through `is_ubiquitous_method_name`, a second axis of
  complexity this ADR does not have a measured case for yet. Exposure
  is uneven across languages: Go is largely shielded because the
  stoplist is all-lowercase while exported interface methods are
  capitalized, but TypeScript idiomatically uses lowercase method
  names that sit on the stoplist (`get`, `set`, `has`, `delete`,
  `clear`), so a TS interface-to-implementation link is the likeliest
  place to lose an edge. Tracked in
  [#230](https://github.com/hiro-o918/rinkaku/issues/230).
- `ExtractedSymbol`'s field addition is mechanical but wide: every
  in-repo constructor of the struct (~20 test call sites plus
  `extract::build_symbol` itself) needs the new field.
- The `deps.rs` candidate-enumeration gap (Decision point 5) remains
  open; tracked as
  [#227](https://github.com/hiro-o918/rinkaku/issues/227).
