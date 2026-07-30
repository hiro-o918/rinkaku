# 0063. Extract macro-body references and count transitive test coverage

- Status: accepted
- Date: 2026-07-30
- Relates to: [ADR 0059](0059-test-coverage-per-symbol.md) (per-symbol
  test coverage; its amendment scoped this follow-up), [ADR
  0042](0042-exclude-test-referrers-from-fan-in.md) (`Node::is_test`),
  [ADR 0003](0003-name-based-dependency-resolution.md) (name-only
  reference resolution), [ADR 0013](0013-hotspots-fan-in-section.md)
  (fan-in aggregation)

## Context

ADR 0059's amendment measured its `test_count == 0` signal at a 67%
false-positive rate in whole-repo outline mode (513 of 763 symbols
reported zero coverage in a repo with 1600+ passing tests) and traced
it to three mechanisms. Two are fixable inside the current name-based
resolution model and were left to this follow-up:

- **References inside macro bodies are not extracted.** The Rust
  reference query matches `call_expression`/`type_identifier` nodes,
  but everything inside a macro invocation is a flat `token_tree` of
  raw tokens — `assert_eq!(inner(), 2)` contains no `call_expression`,
  so `inner` gets no edge. This repo's own house idiom
  (`pretty_assertions::assert_eq!` on whole values) means most test
  bodies reference their subject *only* from inside a macro.
- **No transitive closure.** Coverage counted direct references only:
  a test calling `outer()`, which calls `inner()`, left `inner` at
  `test_count: 0` even though the test exercises it.

The third mechanism — diff scope, where a symbol's tests exist but are
outside the diff — is inherent to diff mode's "graph over changed
symbols" contract and stays out of scope here.

Empirically (tree-sitter-rust), a macro body parses as nested
`token_tree` nodes whose leaves are plain tokens: identifiers (type
names included — `Config::V1` lexes as `identifier "Config"`,
`identifier "V1"`), operators, and literals. Attribute arguments
(`#[cfg(test)]`, `#[derive(Debug)]`) are *also* `token_tree`s, but
under an `attribute` node, not a `macro_invocation`.

## Decision

Two changes, one per mechanism.

**1. Walk `macro_invocation` token trees during Rust reference
extraction.** `extract::collect_referenced_names` additionally walks
the subtree for `macro_invocation` nodes and collects the identifier
tokens inside their `token_tree`, at any nesting depth, into
`referenced_names` — subject to the same `is_noise_name` filter as
query captures. Two token classes are skipped:

- the macro's own name (the `macro` field is not part of the walked
  `token_tree`), and
- any identifier immediately followed by a `!` token — a *nested*
  macro's name (`matches` in `assert!(matches!(...))`), which is a
  macro reference, not a symbol reference.

This is a code walk in `extract.rs`, not a query pattern, for two
reasons: a query has no way to express "identifier *not* followed by
`!`", and the nested-depth pattern `(token_tree (token_tree
(identifier)))` would also match attribute arguments, fabricating
references named `test`, `feature`, or every derive name in the file.
Matching on the `macro_invocation` node kind in language-neutral code
follows `extract::symbol_kind`'s existing precedent (node kind strings
are unique across the supported grammars, so the walk is inert for Go,
Python, and TypeScript — none of which have an equivalent construct).

The new names flow into `referenced_names` itself, not a
coverage-only side channel, so dependency resolution (`deps.rs`),
graph edges, fan-in, and test coverage all see them. This is
deliberate: a macro-body reference is a real reference, and fan-in had
the same blind spot (a symbol called only via `matches!`/`write!` in
production code was invisible to it). One graph, one definition of
"references".

**2. Count transitive coverage in `compute_test_coverage`.** Instead
of keeping only direct test→target edges, traverse the reference graph
from each test node (following outgoing edges through any intermediate
node, cycles handled by a visited set) and record that test in the
`covering_tests` of every non-test node it reaches. A test that
exercises `outer()` genuinely executes `inner()`; reachability
over-approximates execution, but for a signal whose job is flagging
*zero* coverage, a false "covered" is the cheaper error — the ADR 0059
amendment showed the opposite bias destroys the signal entirely.

The output shape (JSON `test_coverage` array, `TestCoverage` struct)
is unchanged; direct and transitive coverage are not distinguished.
The Markdown heading stays `## Changes with no referencing tests` — a
transitive reach is still a reference chain.

`compute_fan_ins` stays direct-only: fan-in measures blast radius of a
signature change, which propagates one edge at a time and is already
rendered transitively by the tree view; only the coverage question is
inherently transitive ("does *any* test end up here").

## Alternatives

- **Query-only extraction of macro-body identifiers**: rejected above
  — cannot skip nested macro names, and the depth-generic pattern
  leaks attribute arguments (`cfg`/`derive` contents) into references.
- **Count macro-body references for coverage only, not fan-in/deps**:
  rejected — two divergent definitions of "reference" over the same
  graph, and the fan-in blind spot is the same bug wearing a different
  aggregation.
- **Depth-capped transitive walk**: rejected — the graph is small
  (hundreds of nodes), a visited set already bounds the walk, and any
  cap reintroduces arbitrary false zeros just beyond the cap.
- **LSP-based resolution**: the eventual precise answer (the
  `Resolver` trait is the plug point), but a much larger dependency
  and out of scope for closing a measured 67% gap with the tools
  already in hand.
- **Fixing the diff-scope mechanism**: would require parsing files the
  diff never touched, breaking diff mode's cost model. Repo-outline
  mode already provides the full-graph answer.

## Consequences

- **Output changes** (pre-1.0 breaking, consistent with ADR
  0013/0042/0059 precedent): `referenced_names`-derived surfaces —
  "Depends on" lists, fan-in counts, graph edges, test coverage — may
  all grow. Existing expectation-pinning tests are updated in the same
  change.
- Macro-body identifiers are collected without call-shape filtering,
  so plain variable names inside macro bodies (`assert_eq!(expected,
  actual)`) enter `referenced_names`. Under name-only resolution (ADR
  0003) they only become edges when a same-named symbol exists in the
  graph — the accepted imprecision baseline since ADR 0003.
- Measured on this repo's whole-repo outline (same tree, same flags,
  main build vs. this change): the zero-coverage rate drops from 522
  of 784 (67%) to 172 of 784 (22%). Symbols the ADR 0059 amendment
  called out as false positives — `entry_row_line`, `build_tree`,
  `render_markdown`'s caller `render` — now report their real
  coverage.
- The dominant *remaining* false-positive mechanism is now visible:
  cross-module scoped calls. `render(...)` reaching `render_markdown`
  via `markdown::render_markdown(report)` produces no edge, because
  the reference query captures only the path head of a
  `scoped_identifier` — the same deliberate decision that keeps
  `Format::default()` from fabricating references to every changed
  symbol named `default` (the two shapes are syntactically
  indistinguishable without type resolution). Extending capture to the
  scoped name is a precision/noise tradeoff (`new`, `default`, `get`
  collide across files) left to a follow-up ADR; the rest of the
  surviving 22% is genuinely test-light wiring (`main.rs`, TUI event
  glue).
- The ADR 0059 amendment's decision to keep the `!` risk-marker
  escalation withdrawn is unchanged: at a measured 22% false-zero rate
  (`symbol_badges` still reads zero through the scoped-call gap), the
  signal is not yet clean enough for the TUI's scarcest visual
  channel.
