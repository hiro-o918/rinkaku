# 0059. Surface per-symbol test coverage alongside fan-in

- Status: Proposed
- Date: 2026-07-20
- Relates to: [ADR 0013](0013-hotspots-fan-in-section.md) (fan-in
  aggregation), [ADR 0042](0042-exclude-test-referrers-from-fan-in.md)
  (`Node::is_test`, test-referrer exclusion from fan-in), [ADR
  0023](0023-tui-blast-radius-naming.md) (blast radius), [ADR
  0043](0043-tui-risk-oriented-visual-encoding.md) (risk-oriented visual
  encoding)

## Context

ADR 0042 made fan-in answer "what breaks in production if I touch
this?" by excluding test referrers from the aggregate, on the grounds
that a symbol's own tests are a reason for *less* concern, not more.
That ADR's Consequences section anticipated the next question this
raises but left it unanswered: fan-in (rightly) no longer tells a
reviewer whether a changed symbol has any tests at all, and a symbol
with zero tests but three production callers is exactly the risky case
ADR 0042 called out — yet nothing in Markdown, JSON, or the TUI
surfaces that emptiness today. The only way to find out is to open the
blast-radius view (ADR 0023) for one symbol at a time and check by eye
whether any of `edges` come from an `is_test` node.

This matters most for the review style rinkaku is built for: skimming
a changed symbol's signature and contract-change badge instead of
reading its body, and trusting its test suite to cover the behavior
instead. That trust is only well-placed when the symbol actually has
tests. Today rinkaku can show *that* a symbol's contract changed
(ADR 0014) but not whether the change is backed by anything — the two
questions "is this risky" (fan-in, contract) and "is this safe to
trust without reading" (test coverage) are answered by different
signals, and only the first is visible in aggregate.

The data already exists: `SymbolGraph::edges` retains every edge
regardless of referrer (ADR 0042 was explicit about not touching
`edges` itself, precisely so the blast-radius view keeps working), and
`graph::Node::is_test` classifies each node using the same rule
`pipeline::partition_test_symbols` (ADR 0009) already uses. What's
missing is an aggregate, symmetric to `compute_fan_ins`, that answers
"which tests cover this symbol" for every changed symbol at once, plus
a place to render it.

## Decision

Add a `compute_test_coverage` aggregation in `rinkaku-core`, alongside
`compute_fan_ins`, that groups `graph.edges` by target node the same
way, but keeps only edges whose referrer *is* `is_test` (the mirror
image of `compute_fan_ins`'s filter). For each changed, non-test node,
this produces a `covering_tests: Vec<NodeId>` list and a `test_count:
usize`. Symbols with `test_count == 0` are the signal this ADR exists
to surface.

Rendering, one per output surface:

- **TUI**: `Badges` already computes fan-in and contract-change counts
  per symbol (`tree::symbol_badges`) and merges them upward into
  `Dir`/`File` aggregates (`Badges::merge`); add a `test_count` field
  the same way. Rendering it is **new**, not an extension of the
  existing `chg:`/`api:`/`fan-in:` badge row: `push_badge_spans` only
  ever runs for `BadgeContext::Dir`/`File` — `entry_row_line`'s
  `NodeKind::Symbol` arm never calls it, and a symbol row today shows
  only a risk marker, kind abbreviation, and name
  (`push_badge_spans`'s own doc comment: "symbol rows have their own
  layout, no badge summary"). This ADR adds a small `tests:0` badge to
  that `Symbol` arm directly, shown **only when `test_count == 0`**
  (the inverse of the existing badges' "only nonzero counters render"
  rule — here zero is the signal, so most symbol rows, which do have
  coverage, stay exactly as terse as today). A `SignatureChanged`
  symbol with `tests:0` gets the same `!` risk marker treatment
  `is_high_risk_symbol` already applies to `SignatureChanged` +
  high-fan-in (`row_view.rs`) — untested contract changes are at least
  as worth a second look as widely-referenced ones.
- **Markdown**: add a `covering_tests` count next to each definition
  entry, and a `## Untested changes` section listing every changed,
  non-test symbol with `test_count == 0`, mirroring the shape of the
  existing `## High fan-in symbols` section (ADR 0013). Unlike
  fan-in, today's `render_definition` has no existing inline count to
  follow as precedent for the per-definition placement — exact
  placement (heading line vs. before the signature block) is an
  implementation detail to settle during the change, not fixed by this
  ADR.
- **JSON**: add `test_coverage: [{ symbol, test_count, covering_tests:
  [...] }]` as a new top-level array, parallel to the existing
  `fan_ins` array, so a consumer that only wants the empty-coverage
  set can filter `test_count == 0` without recomputing anything.
- **Mermaid**: no change in this ADR. Mermaid's node labels are already
  dense (ADR 0039) with the `(in:N)` fan-in suffix and diff markers
  (ADR 0041); a `tests:N` suffix competes for the same space. Mermaid
  also serves a different job than this ADR's TUI/Markdown targets — a
  human-skimmable overview for PR comments/descriptions (ADR 0021),
  not the per-symbol "can I trust this without reading it" judgment
  this ADR is for. Left for a follow-up if there's demand.

This applies regardless of `--exclude-tests`: under that flag, test
symbols are dropped from the graph entirely (ADR 0009/0025), so
`compute_test_coverage` naturally reports `test_count: 0` for every
symbol — the same "no visible test coverage" signal, for the same
reason (there is no data to say otherwise). No special-casing needed.

## Alternatives

- **Rely on the existing blast-radius view alone**: already shows
  "this test exercises the symbol you're viewing" per ADR 0042, one
  symbol at a time. Rejected as the sole mechanism: a reviewer working
  through a multi-symbol diff needs to spot the *untested* ones without
  opening blast radius for each one individually — an aggregate,
  scannable in one screen, is the point.
- **Revert ADR 0042 and let fan-in include tests again**: would make a
  well-tested symbol's fan-in rise, restoring visibility into test
  coverage as a side effect. Rejected for the reason ADR 0042 gives:
  it re-inverts the fan-in signal (well-tested looks *more*
  dangerous), trading one blind spot for the exact problem 0042 fixed.
- **A single boolean (`has_tests`) instead of a count/list**: cheaper
  to render, but throws away `covering_tests`, which lets a reviewer
  jump straight to the relevant test (useful in the TUI: selecting a
  `tests:N` badge could pivot to those rows, symmetric to blast
  radius). Rejected — the count and list cost little more to compute
  since `compute_fan_ins`'s edge-grouping is reused as-is, just with
  the filter inverted.
- **Fold into fan-in as `production_fan_in` / `test_fan_in` pair**:
  ADR 0042 already rejected a parallel-fields design for the same
  aggregation for a different reason (nobody needs unfiltered fan-in).
  This proposal isn't reviving that: test coverage is a different
  question from fan-in ("who tests this" vs. "what breaks in prod"),
  not an alternate cut of the same one, so it earns its own aggregate
  rather than riding along inside `FanIn`.

## Consequences

- **Breaking change** to Markdown (new `## Untested changes` section),
  JSON (new `test_coverage` array), and the TUI (new `tests:0` badge on
  symbol rows, shown only for uncovered symbols, and transitively the
  `!` risk marker for untested contract changes). Consistent with this
  project's pre-1.0 stance on output-format changes (ADR
  0013/0034/0042's own precedent).
- Reuses `Node::is_test` exactly as ADR 0042 anticipated ("available
  for any future aggregation that needs the same production/test
  split") — no new classification logic, no new `LanguageSupport`
  surface.
- `compute_test_coverage` is a straightforward mirror of
  `compute_fan_ins`'s existing edge-grouping, so the implementation
  risk and review surface is small; this is a medium change per
  CLAUDE.md's process-weight guidance (a new pure function plus a
  rendering rule), not a large one — no dogfooding three-angle review
  required, one map-assisted pass is enough.
- Under `--exclude-tests`, every symbol reports `test_count: 0` by
  construction (no test data survives that flag). This is expected and
  documented above, not a bug to special-case.
- Sets up, but does not implement, a natural follow-up: a
  `--fail-on-untested-contract-change` CI gate once the Action
  (ADR 0036) wants to enforce "contract changes ship with tests" rather
  than just surface it for human/LLM review.
