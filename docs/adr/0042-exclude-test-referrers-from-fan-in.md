# 0042. Exclude test referrers from fan-in counts

- Status: accepted
- Date: 2026-07-14
- Amends: [ADR 0013](0013-hotspots-fan-in-section.md) (fan-in aggregation
  definition), [ADR 0034](0034-rename-hotspots-vocabulary-to-fan-in.md)
  (naming, `HIGH_FAN_IN_THRESHOLD`)

## Context

`compute_fan_ins` (ADR 0013, renamed by ADR 0034) aggregates
`graph.edges` by target node, counting every distinct referrer toward a
symbol's fan-in without regard to whether the referrer is production
code or a test. ADR 0025 defaults tests into the same graph as
production symbols, so a symbol exercised by several `#[test]`/
`should_...` functions accumulates fan-in from those edges exactly like
it would from real callers.

Fan-in exists to answer "what breaks if I touch this?" (ADR 0013's
"question B") — a proxy for production blast radius. Test referrers
invert that signal instead of reinforcing it: a symbol thoroughly
covered by its own tests looks *more* dangerous to touch under the
current aggregation, when in practice the opposite holds — its tests
are rewritten alongside it as a matter of course when it changes, and
good coverage is a reason for *less* concern, not more. A symbol with
zero tests but three production callers is the actually risky case, and
today it can rank *below* a well-tested leaf symbol purely because the
latter has more referring test functions.

This does not touch `SymbolGraph::edges` itself or ADR 0035's tree
placement (whole test files still sort into the trailing "Tests"
section; mixed-file test symbols are unaffected in the graph). It is
scoped to the fan-in aggregation step alone.

## Decision

- Add `graph::Node::is_test: bool`, computed in `collect_nodes` from the
  same rule `pipeline::partition_test_symbols` (ADR 0009) already uses:
  true if the owning file is a whole test path
  (`LanguageSupport::is_test_path`) or the symbol's own AST context
  marks it as a test (`ExtractedSymbol::is_test`). `#[serde(skip)]` —
  not part of any output shape, same treatment ADR 0009's original
  `is_test` field on `ExtractedSymbol` already received, for the same
  reason: it is an internal aggregation input, not a fact the Markdown/
  JSON/Mermaid consumer needs to see per node.
- `compute_fan_ins` skips any edge whose referrer (`edge.from`) resolves
  to an `is_test` node before grouping by target and applying
  `HIGH_FAN_IN_THRESHOLD`. A target's fan-in count and `used_by` list
  now reflect only production referrers.
- `SymbolGraph::edges` is unchanged — every edge, test-referrer or not,
  is still built and still serialized. The TUI's blast-radius view
  (`crate::blast_radius`, ADR 0023) walks `edges` directly, so it
  continues to show "this test exercises the symbol you're viewing,"
  which is true and useful there; only the fan-in *aggregate* excludes
  it.

## Alternatives

- **Drop test-referrer edges from `SymbolGraph::edges` entirely**:
  would fix fan-in for free by construction, but breaks the
  blast-radius view's ability to show "these tests cover this symbol"
  — a fact a reviewer legitimately wants when assessing whether a
  change is safe. Rejected: fan-in and blast-radius answer related but
  distinct questions (aggregate risk ranking vs. "show me everything
  that touches this one symbol"), and the fix belongs in the narrower
  consumer (fan-in), not in the shared data both consumers read.
- **Filter at render time (Markdown/JSON/Mermaid each skip test
  `used_by` entries)**: would need the same filter duplicated in three
  render modules instead of once in `compute_fan_ins`, and would leave
  the TUI badge aggregate (`Badges.fan_in`, which reads `FanIn` values
  computed once in `rinkaku-core`) still counting test referrers unless
  it re-filtered too — same duplication, one more place. Rejected in
  favor of filtering once at the aggregation source every consumer
  already shares.
- **A separate `production_fan_in` field alongside the existing
  (unfiltered) fan-in**: considered, to preserve both signals. Rejected
  as solving a problem nobody asked for — no consumer has a use for
  "fan-in including tests," and carrying both would just require every
  render surface to pick the right one instead of removing the wrong
  one.

## Consequences

- **Breaking change** to Markdown ("High fan-in symbols" section
  membership and `used_by` list), JSON (`fan_ins[].used_by`), Mermaid
  (`fan-in` node classification and `(in:N)` label suffix, ADR 0039),
  and the TUI (`fan-in:N` badge, and transitively [ADR 0043](0043-tui-risk-oriented-visual-encoding.md)'s
  `!` risk marker, which reads the same test-excluded count). Consistent
  with this project's established pre-1.0 stance on output-format
  changes (ADR 0013/0034's own precedent).
- A symbol whose only referrers are tests now has fan-in 0 and never
  appears in "High fan-in symbols"/`fan_ins`, even if it has many test
  callers — expected, since "many tests call this" is no longer the
  question fan-in answers.
- `Node::is_test` is available for any future aggregation that needs
  the same production/test split (e.g. a hypothetical "test-only
  hotspot" metric), without re-deriving the rule a third time; the
  authoritative rule stays `pipeline::partition_test_symbols` and
  `LanguageSupport::is_test_path`, per ADR 0009.
