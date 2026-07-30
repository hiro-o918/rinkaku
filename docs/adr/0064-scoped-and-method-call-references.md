# 0064. Capture module-scoped and method-call references, with a ubiquitous-name stoplist

- Status: accepted
- Date: 2026-07-30
- Relates to: [ADR 0063](0063-macro-body-references-and-transitive-test-coverage.md)
  (whose Consequences section scoped this follow-up), [ADR
  0003](0003-name-based-dependency-resolution.md) (name-only
  resolution), [ADR 0059](0059-test-coverage-per-symbol.md) (test
  coverage), [ADR 0013](0013-hotspots-fan-in-section.md) (fan-in)

## Context

After ADR 0063, the whole-repo zero-coverage rate stood at 22%, and its
dominant remaining cause was measured to be two reference shapes the
Rust query deliberately does not capture:

- **Module-scoped calls.** `render(...)` reaches `render_markdown` via
  `markdown::render_markdown(report)`, but the query captures only the
  path head of a `scoped_identifier` — so the call chain from every
  test into `markdown.rs` broke at that hop, reporting 22 well-tested
  helpers as uncovered.
- **Method calls.** `x.bar()` parses as `function: (field_expression)`,
  never captured, so method-heavy code (the TUI in particular —
  `build_tree` reaches `build_file_node` through a
  `dir_tree.into_nodes(...)` hop) lost its chains the same way.

Both exclusions existed for a good reason: the same syntax also spells
associated-function and std-trait calls, where name-only resolution
(ADR 0003) fabricates edges. This ADR is the outcome of measuring that
tension on this repo (whole-repo outline, 791 non-test symbols, 1733
tests passing at the time of measurement):

| variant | zero-coverage | collateral |
|---|---|---|
| baseline (post-0063) | 174/788 (22.1%) | — |
| capture every scoped-call name | 113/788 (14.3%) | every `fn new` node at `used_by` 142 (from 5); edges into `new`/`from`/`read` 1201 → 8804 |
| + lowercase-path heuristic | 127/790 (16.1%) | +8 edges to those names; `new` stays at 5 |
| + method names, unfiltered | 48/790 (6.1%) | a repo `fn clone` drew 143 referrers, `fn get` 138, `fn next` 23 — all three entered high fan-in |
| + method names behind stoplist (this ADR) | 52/791 (6.6%) | top fan-in identical to baseline's legitimate list (`Report` 53) |

## Decision

Three parts, all Rust-only (the other grammars have no equivalent
ambiguity):

1. **Module-scoped call names are captured in a code walk**
   (`extract::collect_module_scoped_call_names`) when the path segment
   is `super`, `crate`, or a lowercase-initial identifier — Rust's
   module naming convention is the only signal separating
   `markdown::render_markdown(x)` from `Format::default()`, and a
   tree-sitter query cannot test identifier case, so this lives in code
   (the ADR 0063 macro-walk precedent). Capitalized paths keep the
   existing behavior: path captured as a type, name left unresolved.
   Nested paths (`std::fs::read`) stay uncaptured.
2. **Method-call names are captured in the query** under a distinct
   `@reference.method` capture, filtered through a stoplist of
   ubiquitous std trait/idiom method names
   (`extract::is_ubiquitous_method_name`: `clone`, `get`, `next`,
   `len`, `map`, `unwrap`, ...). The filter keys on the capture name,
   so the same identifier called as a free function (`get(1)`) or
   defined as a symbol is unaffected. The membership criterion is
   "belongs to a std trait or ubiquitous std method idiom", not
   "observed colliding in this repo" — pollution appears the day a
   same-named symbol enters a graph. Extending the list is a routine
   change; changing the criterion is an ADR amendment.
3. As in ADR 0063, the new names flow into `referenced_names` itself —
   deps, graph edges, fan-in, and coverage all see them. One graph, one
   definition of "references".

## Alternatives

- **Capture scoped/method names unconditionally**: rejected by the
  measurements above — both variants push fabricated referrers into
  exactly the aggregations (fan-in, coverage) this line of work exists
  to make trustworthy.
- **Heuristics in the query via `#match?` predicates**: the
  `tree-sitter` crate's cursor API does not evaluate predicates; the
  filtering would silently not happen.
- **Capture method references for coverage only**: rejected in ADR 0063
  already — two divergent definitions of "reference" over one graph.
- **Type-aware resolution (LSP)**: the eventual precise fix for both
  ambiguities (the `Resolver` trait is the plug point); out of scope
  for what a naming-convention heuristic and a stoplist close today.

## Consequences

- Whole-repo zero-coverage drops 22.1% → 6.6%, and the survivors are
  dominated by genuinely test-light wiring (`main.rs`, event loop,
  clipboard) rather than well-tested core code. `symbol_badges` — the
  ADR 0059 amendment's remaining named false positive — now reports its
  real coverage (276).
- Output changes (pre-1.0 breaking, ADR 0013/0042/0059/0063
  precedent): "Depends on" lists, fan-in, and coverage grow wherever
  macro-free code calls through modules or methods. Measured top
  fan-in is unchanged from baseline's legitimate list.
- The stoplist trades away coverage visibility for repo symbols that
  share a std method name and are only ever called as methods — a
  repo's own `fn clone` impl can no longer be linked from its callers.
  Accepted: such a symbol's 143-referrer alternative was worse, and
  its tests still reach it through any non-method reference.
- A lowercase type name or a capitalized module would defeat the path
  heuristic; both violate Rust naming conventions loudly enough that
  the mis-capture is accepted as noise of the same order ADR 0003
  already carries.
- The `!` risk-marker escalation (withdrawn in ADR 0059's amendment)
  stays withdrawn even at 6.6%: in diff mode — the TUI's primary input
  — the diff-scope mechanism still reports `tests:0` for any symbol
  whose tests exist but lie outside the diff, so the marker would
  still fire falsely on routine PRs. Re-evaluate if a future change
  closes the diff-scope gap.
