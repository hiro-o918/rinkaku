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

## Results (round 3)

Same two-pass method, third subject: TUI iteration 2 (the `d`-key diff
pane and directory/file detail views; 8 files, +2,016/−118 — a
brownfield diff layering a new pure module and view-models onto the
iteration-1 TUI). One protocol change from rounds 1–2: dynamic
verification was **mandatory for both arms** this round (per the
CLAUDE.md rule adopted after round 2), so both agents built and
executed the binary rather than leaving execution to chance.

| Metric              | A (map)     | B (control) |
| ------------------- | ----------- | ----------- |
| Output tokens       | 173.4k      | 173.2k      |
| Tool calls          | 38          | 51          |
| Wall clock          | 301s        | 413s        |
| Blocker findings    | 0           | 0           |
| Should-fix findings | 0           | 2           |
| Nit findings        | 2           | 1           |

- **Only B** found both should-fix issues, and both are the same
  *class*: line-level behavior invisible on the signature surface.
  (1) The diff pane re-parsed the entire raw diff text inside the
  render closure — roughly ten times per second on idle poll ticks —
  found by reading the event-loop body. (2) The new hunk parser
  trusted the `@@` header's declared line count without validating it
  against the actual body, silently diverging from
  `rinkaku-core::diff::parse_hunk`'s strict `HunkBodyMismatch`
  contract on the same failure mode; found by *comparing the two
  parsers side by side*, an association the map draws no edge for.
- **Only A** found the self-consistency defect: `build_dir_detail`
  called `cycle_partners` and `cycle_edges` as two independent entry
  points, each rebuilding the Tarjan condensation that
  `DirCondensation`'s own doc comment says exists to be built once.
  The map's dependency arrows under `build_dir_detail` (two arrows
  into `order.rs`) are what prompted reading both callees' bodies —
  attention allocation working as designed, though the defect itself
  still required reading past the signatures.
- A's second unique finding (no scroll support in the detail/diff
  panes, a pre-existing gap the new whole-file diff view makes easier
  to hit) came from its **dynamic-verification step**, not the map —
  evidence that making execution mandatory for both arms, rather than
  a lucky extra, pays off. With both arms executing, round 2's
  asymmetry (only the executing arm finding the runtime defect) did
  not recur; neither arm found any runtime defect this round, and
  their live checks independently confirmed the same behaviors.
- A third angle outside both arms — the orchestrator's own fixture
  testing during dynamic verification — surfaced a limitation neither
  reviewer saw: name-match edge collection produces **no edges for
  qualified cross-package references** (e.g. Go's `store.Save()`), so
  the new cycle explanation can be silently empty for Go code. A
  pre-existing core behavior, not a defect of this diff, but it
  bounds the feature's usefulness per language and neither static
  pass had a reason to test it.
- Token costs converged this round (under 0.2% apart); B spent ~34%
  more tool calls and wall clock, mostly on its parser-comparison and
  convention sweeps.
- All four findings were fixed before merge; fixing the hunk-count
  finding immediately caught a miscounted header in one of the new
  module's own test fixtures — small, direct evidence the defensive
  check pays for itself.

## Conclusions (after 3 rounds)

- Three rounds, one stable pattern: the map arm keeps winning on
  **self-consistency and cross-module contracts** (doc/impl drift in
  round 2, a module contradicting its own stated rationale in round
  3), while the unassisted arm keeps winning on **line-level and
  behavioral findings** (conventions in round 1, the non-TTY panic in
  round 2, the render-loop re-parse and parser divergence in round
  3). Neither arm has ever produced a superset of the other; the
  two-pass default stands.
- Making dynamic verification mandatory for both arms removed round
  2's luck factor without erasing the arms' complementary profiles —
  the differentiation comes from *reading strategy*, not from who
  executes.
- New hypothesis for the tool itself (from round 3's should-fix #2):
  duplicated responsibility between crates (two unified-diff parsers)
  is a defect class the map could surface directly — e.g. by flagging
  same-named or same-domain symbols appearing in multiple crates —
  where today it needs a reviewer to happen to know both sides exist.

## Results (round 4)

Same two-pass method (dynamic verification mandatory for both arms),
fourth subject: whole-repo mode per ADR 0017 (4 files, +710/−29 — a
new pure core entry point `analyze_repo`, a mode-selection change in
`main.rs`, and a `Format` → `Option<Format>` CLI refactor rippling
through 19 test literals).

| Metric              | A (map)     | B (control) |
| ------------------- | ----------- | ----------- |
| Output tokens       | 98.0k       | 100.5k      |
| Tool calls          | 37          | 39          |
| Wall clock          | 288s        | 274s        |
| Blocker findings    | 0           | 0           |
| Should-fix findings | 2           | 0           |
| Nit findings        | 1           | 2           |

- **First cross-arm overlap** in four rounds: both arms found the
  redundant `is_none()` guard in the new input-mode branch — but
  rated it differently. A rated it should-fix because it read the
  surrounding comment and noticed it describes a provably unreachable
  path (and that the stdin TTY bail it references had become dead
  code); B rated the same redundancy a harmless nit. Same line,
  different severity, decided by how much *surrounding contract* each
  reviewer read — a reminder that finding overlap does not mean
  judgment overlap.
- A's second should-fix (whole-repo output saying "N changed symbols"
  when nothing changed) and its nit (raw `git ls-files` error on the
  new default invocation path) both came from **executing the
  binary**, not from the map — as did both of B's nits (silent empty
  output on an empty repository; deleted-file-between-list-and-read
  resilience, which passed). With both static surfaces reviewed
  clean, dynamic verification produced every unique finding this
  round, on both arms.
- The map's contribution this round was **negative space**: A
  reported the map's dependency listing under `fn main` told it which
  helpers were pre-existing and not worth re-auditing, and its
  hotspots sent it straight to the `analyze_repo` ↔ `TagsResolver`
  filter-order parity check (which came back clean, verifying the
  ADR's core claim cheaply). Attention allocation showed up as
  *cheaper verification of the contract that mattered*, not as
  unique findings.
- Round 3's arm profiles partially inverted: the map arm won the
  line-level control-flow finding this round. The stable pattern
  across four rounds is not "map = architecture, control =
  line-level" but weaker and more durable: **the arms rarely
  duplicate each other's unique findings, and severity judgment
  varies even on shared ones** — the case for two passes is
  redundancy of judgment, not partition of territory.

## Conclusions (after 4 rounds)

- Two-pass review remains justified, but the mechanism is now better
  understood: independent judgment (severity, contract reading)
  diverges even when raw findings overlap, and each arm keeps
  producing unique findings the other misses.
- Dynamic verification has become the dominant source of unique
  findings as the codebase's static review surface gets cleaner
  (rounds 3–4: every should-fix/nit unique to an arm traced to
  either execution or cross-file contract reading, none to the diff
  text alone). The CLAUDE.md rule making it mandatory for both arms
  is doing the heavy lifting.
- The map's measurable value at this diff size is verification
  routing: it made the ADR's central parity claim (filter order
  matching `TagsResolver::new`) cheap to check and marked
  pre-existing code as safe to skip.

## Results (round 5)

Same two-pass method, fifth subject: right-pane scrolling in the TUI
(4 files, +493/−15 — new scroll state in `App`, draw-time clamping and
an overflow indicator in `ui.rs`). The first subject to contain
genuine blockers.

| Metric              | A (map)     | B (control) |
| ------------------- | ----------- | ----------- |
| Output tokens       | 115.2k      | 108.1k      |
| Tool calls          | 66          | 53          |
| Wall clock          | 558s        | 397s        |
| Blocker findings    | 2           | 2           |
| Should-fix findings | 1           | 2           |
| Nit findings        | 0           | 1           |

- **Both arms independently found — and live-reproduced — the same
  two blockers**: (1) the scroll clamp and indicator count logical
  lines while ratatui's `Paragraph` scrolls wrapped rows, so any
  wrapped long line makes tail content permanently unreachable while
  the indicator claims everything is shown; (2) collapse/expand/select
  never reset the scroll offset even though a collapse can re-target
  the cursor to a different node, opening the new content pre-scrolled
  past its own header. First full cross-arm convergence in five
  rounds, and it happened on the first subject with real blockers —
  encouraging evidence that the process converges on defects that
  matter regardless of reading strategy.
- Both discoveries were made by **interacting with the running
  binary** (narrow terminals, scroll-then-collapse sequences), then
  explained by code reading. The orchestrator's own third-angle
  spot-check had exercised only short lines and cursor-reset paths
  and pronounced the feature working — a concrete demonstration that
  a hands-on smoke test by the coordinating agent is not a substitute
  for adversarial reviewers with time to probe edges.
- Map metadata cuts both ways this round: the map's dependency edges
  under `draw_detail_pane` pointed A at `cycle_edges`' full-path lines
  — exactly the realistic trigger for blocker 1 — but the defect's
  *mechanism* lives in the ratatui dependency's wrap-vs-scroll
  semantics, outside anything a change map can show. And blocker 2's
  mechanism (`nav.rs`'s cursor re-targeting) sits in **unchanged**
  code, which the change map by design does not draw — the map
  actively lacked the one edge that made the regression provable. A
  change-scoped map cannot flag regressions whose mechanism is
  pre-existing; only reading beyond the diff does.
- Unique findings were extensions rather than independent defects:
  A added the `ToggleOrder` reset gap (same root cause as blocker 2);
  B added missing-test observations and a clone-per-frame nit.
- Both blockers (and the reset gap) were fixed before merge; the fix
  re-verified against each pass's original reproduction steps.

## Conclusions (after 5 rounds)

- On the first subject with genuine blockers, both arms found both,
  independently and with live reproductions — the two-pass design's
  redundancy paid off as confidence, not just coverage.
- The strongest single predictor of finding real defects across all
  five rounds is **hands-on execution with hostile inputs** (narrow
  terminals, malformed diffs, non-TTY environments), not reading
  strategy. The map's role stays what round 4 concluded:
  verification routing and attention allocation.
- New limit identified: the change map cannot surface regressions
  whose mechanism lies in unchanged code or in a dependency's
  semantics — both blockers this round had exactly that shape.
  Reviewer instructions should explicitly license reading beyond the
  diff (and the CLAUDE.md three-angle policy already does).

## Next

- Consider a map feature flagging cross-crate duplicate-domain
  symbols (round 3 hypothesis, still open).
- The round-5 blockers suggest a second tool hypothesis: listing the
  *unchanged* symbols that changed symbols newly depend on (the map
  currently draws edges only among changed/1-hop symbols) might have
  surfaced `retarget_cursor` as blast-radius context for `handle_key`.
