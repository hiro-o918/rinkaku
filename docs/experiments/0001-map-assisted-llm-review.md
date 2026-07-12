# Experiment 0001: rinkaku change maps as an entry point for LLM code review

- Status: round 1 complete, round 2 planned
- Date: 2026-07-12

## Hypothesis

Feeding an LLM review agent rinkaku's own output (change graph +
fan-in hotspots + contract markers, per ADR 0013/0014) as a "map"
before it reads the diff improves review quality — specifically token
efficiency, finding accuracy, and architecture-level insight — because
the agent can allocate deep-reading attention instead of
reconstructing the change structure itself.

## Method

Two review agents with the **same model, same effort setting, and a
character-identical prompt** (review criteria, output format, severity
scale) reviewed the same diff:

- **A (map-assisted)**: additionally instructed to first read the
  rinkaku output for the diff (generated with a trusted build from
  `main`, not from the branch under review) and derive its own
  deep-reading priorities from it. Explicitly told the map does not
  excuse skipping unmapped code.
- **B (control)**: no map.

Subject: a ~2k-line greenfield diff — a new pure view-model crate
(directory tree building, SCC-based topological ordering, navigation
state machine, detail view) with no modifications to existing crates.
Nearly every symbol carried a `— new` marker, so contract markers had
little discriminating power; the informative parts of the map were the
entry-point trees and the fan-in hotspot ranking.

## Results (round 1)

| Metric              | A (map)     | B (control) |
| ------------------- | ----------- | ----------- |
| Output tokens       | 81.6k       | 84.8k       |
| Tool calls          | 22          | 22          |
| Wall clock          | 229s        | 213s        |
| Critical findings   | 2           | 1           |
| Should-fix findings | 3           | 3           |

Finding overlap and uniqueness:

- **Both** found the same genuine critical: a navigation-state cursor
  left unclamped after collapse actions, panicking under the API usage
  pattern the doc comment itself recommends.
- **Only A** found the highest-value critical: a cross-module contract
  mismatch — the directory-rank map is keyed by direct parent
  directories, while tree ordering looks up ranks by (possibly
  intermediate or collapsed) node paths, silently degrading
  topological ordering to the A-Z fallback. Each module's unit tests
  passed; only the integration breaks. A's report shows it steered
  there from the map's hotspot ranking (the tree node type referenced
  from 8 places) and framed the whole review around cross-boundary
  contracts.
- **Only B** found the convention-level issues: a library-code
  `.expect()` that the equivalent core code deliberately avoids, and
  partial asserts missing the NOTE comment the test conventions
  require. A had inspected the same `.expect()` and explicitly judged
  it acceptable.

## Interpretation (provisional, n=1)

- **Token savings: none observed** at this diff size. The map's value
  did not show up as fewer tool calls or tokens.
- **Architecture-level review: clear win for the map.** The
  only-A critical is exactly the class of defect (integration seam,
  fan-in-heavy shared type) the map is structurally suited to
  surface, and the class human reviewers care most about.
- **Line/convention-level review: the control did better** in this
  round, though with n=1 this may be ordinary agent variance rather
  than an anchoring effect of the map.
- Working conclusion: the map is a **complement, not a substitute** —
  attach it when architecture and integration seams matter, and keep
  convention-level review as an independent concern.

## Threats to validity

- n=1 per arm; single subject diff; no repeated runs per arm.
- Greenfield diff mutes the contract markers (`signature changed`,
  `removed`), which are expected to matter more on brownfield diffs.
- Token/call counts include each agent's whole session, not just the
  reading phase.
- Findings were adjudicated by the same orchestrator that designed the
  experiment.

## Next

- Round 2 on a brownfield diff (the TUI shell stage, which wires new
  code into the existing binary crate) to test the contract-marker
  side and re-check the convention-level regression.
- If the complement pattern holds, document the map-first recipe in
  the README's LLM usage section.
