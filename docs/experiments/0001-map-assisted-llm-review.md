# Experiment 0001: rinkaku change maps as an entry point for LLM code review

- Status: rounds 1 and 2 complete
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

## Results (round 2)

Same method, second subject: the full TUI branch after the shell stage
(~6.3k added lines; new ratatui shell modules plus wiring into the
existing binary crate — a mixed greenfield/brownfield diff whose map
carried 2 `signature changed` markers and a removed-symbols section).

| Metric              | A (map)     | B (control) |
| ------------------- | ----------- | ----------- |
| Output tokens       | 145.1k      | 103.0k      |
| Tool calls          | 32          | 41          |
| Wall clock          | 303s        | 420s        |
| Critical findings   | 0           | 0           |
| Should-fix findings | 3           | 3           |

- **Only B** found the round's most actionable defect: the terminal
  init call panics (raw panic, exit code 101) in non-TTY environments
  instead of returning an error through the CLI's normal error path.
  B found it by *executing the binary*, spending its extra tool calls
  on dynamic verification — a class of defect the static map cannot
  surface in principle.
- **Only A** found documentation/contract drift: a doc comment
  claiming an invariant ("every action re-targets the cursor") that
  two code paths bypass, and an untested self-referencing-edge
  boundary in the detail view — consistency findings of the kind the
  map's structural framing keeps in view.
- Token pattern reversed from round 1: the map-assisted agent spent
  ~40% more tokens this round. The map neither reliably saves nor
  costs tokens at these diff sizes; it redirects attention.

## Conclusions (after 2 rounds)

- The map is a **complement, not a substitute**, and the complement
  axis is now clearer: map-assisted review is stronger on
  integration seams, cross-module contracts, and doc/impl drift;
  unassisted review left more budget for convention checks (round 1)
  and dynamic execution (round 2), each of which produced the round's
  best unique finding.
- Neither arm dominated on any metric across both rounds; running
  **both passes** is the defensible default, which CLAUDE.md's
  "Reviewing changes" section now requires.
- Token efficiency is not a selling point of the map at PR scale;
  attention allocation is.

## Next

- Optional round 3 with a review pass that combines the map with an
  explicit "execute the binary" instruction, to test whether one
  agent can cover both axes without losing either.
- Document the map-first recipe in the README's LLM usage section
  (map from a trusted build, paired with an independent pass).
