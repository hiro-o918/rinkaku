# Experiment 0001: rinkaku change maps as an entry point for LLM code review

- Status: 22 rounds complete (index/conclusions cover rounds 1-10, 17,
  and 22; rounds 11-16 and 18-21 are recorded under `rounds/` but not
  yet folded in)
- Date: 2026-07-13

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

Starting round 3, dynamic verification (building and executing the
binary against hostile/edge-case inputs) became **mandatory for both
arms**, per the CLAUDE.md rule adopted after round 2. Round 10 is a
recorded exception (dynamic verification assigned to arm B only); see
that round's own protocol note.

## File layout (this document was split from a single growing file)

A single ever-growing experiment file caused merge conflicts whenever
parallel branches each appended a round to it. This directory splits
that file so future rounds are conflict-free:

- **This README**: stable front matter (hypothesis, method above), the
  running "Conclusions" section below, and the round index. **Updated
  only in dedicated follow-up commits on `main`** — not as part of the
  same commit/PR that adds a new round file — so that a round-adding
  PR only *adds* a file (`rounds/NNN.md`) and never touches a line
  another parallel PR might also be touching. If you're adding a new
  round, add `rounds/NNN.md` in your PR and leave this README's
  Conclusions/index update to a separate, later commit (by whoever
  merges next, or a fast follow-up).
- **`rounds/001.md` … `rounds/010.md`**: one file per round, zero-
  padded, append-only. Never edit a past round's file except to fix a
  factual error in what it already records — new interpretation goes
  in a *later* round's file or in this README's Conclusions, not by
  rewriting an earlier round.

## Round index

| Round | Subject | One-line takeaway |
| ----- | ------- | ------------------ |
| [001](rounds/001.md) | New pure TUI view-model crate (greenfield) | Map wins on architecture/integration seams; control wins on convention-level review |
| [002](rounds/002.md) | Full TUI branch after the shell stage | Only the executing arm (control) caught a non-TTY panic; map caught doc/impl drift |
| [003](rounds/003.md) | TUI diff pane + directory/file detail views | Mandatory dynamic verification became the rule; map caught a self-consistency defect, control caught two line-level ones |
| [004](rounds/004.md) | Whole-repo mode (ADR 0017) | First cross-arm overlap on one finding, but with different severity judgments; dynamic verification produced every unique finding |
| [005](rounds/005.md) | TUI right-pane scrolling | First subject with genuine blockers — both arms independently found and reproduced both |
| [006](rounds/006.md) | Diff-pane syntax highlighting (ADR 0018) | First fully clean round on a code subject, reached via convergent adversarial probing |
| [007](rounds/007.md) | Entry-path pivot (ADR 0019) | Doc-versus-implementation drift was the dominant defect shape (4 of 7 findings) |
| [008](rounds/008.md) | TUI source view path resolution fix | Both arms converged on the same blocker (workdir propagation); only dynamic verification caught a narrow-pane wrapping bug |
| [009](rounds/009.md) | TUI entry tree: skipped/test-only files | Zero overlap, zero shared theme — the cleanest complementary-findings round yet |
| [010](rounds/010.md) | This PR's own diff: `--format mermaid` + GitHub Action | Mostly non-Rust surface; map's best signal was naming its own blind spot; neither arm caught a first-PR bootstrap bug the orchestrator found post-merge |
| [017](rounds/017.md) | Minimal `env_logger` config change in `fn main` (PR #79) | Map was correctly silent on a body-only diff whose entire risk lives in rendered log text, not any signature; reviewer fell back to manual consumer tracing, dynamic verification carried the confirmation |
| [022](rounds/022.md) | Tests excluded from entry-tree ranking + trailing Tests section (ADR 0035, PR #112) | Map caught two review-blockers as pure bookkeeping (stale ADR status, a mid-PR file-size warning); dynamic verification found a pre-existing bug (`is_test_path` blind to this repo's own `*_tests/` convention) that no unit test in the PR's 1121-test suite could reach |

Rounds 11-16 and 18-21 are recorded under `rounds/` but not yet folded
into this index; that backlog is tracked separately from this round's
follow-up.

## Threats to validity

- n=1 per arm per round; no repeated runs per arm per subject.
- A greenfield diff (round 1) mutes the contract markers (`signature
  changed`, `removed`), which matter more on brownfield diffs.
- Token/call counts include each agent's whole session, not just the
  reading phase.
- Findings were adjudicated by the same orchestrator that designed the
  experiment.
- Round 7's cross-arm convergence on finding 1 followed the
  orchestrator seeding a specific suspicion into both review prompts —
  not independent evidence that round. Round 10's dynamic-verification
  assignment (arm B only) is a similar, recorded asymmetry.
- Round 17 was not run as a harness-timed A/B pair at all (no token/
  tool-call/wall-clock metrics collected); it records the review this
  PR actually received before merge, in the per-round format used for
  continuity, not as comparable experimental data.
- Round 22, like round 17, was a single-reviewer session recording the
  review a PR actually received before merge, not a harness-timed A/B
  pair; no token/tool-call/wall-clock metrics were collected.

## Conclusions (after rounds 1-10, 17, and 22)

- The map is a **complement, not a substitute**. Across ten rounds,
  neither arm has ever produced a superset of the other's findings;
  running both passes (CLAUDE.md's "Reviewing changes" rule) remains
  the defensible default.
- Token efficiency is not a selling point of the map at PR scale;
  **attention allocation** is. The map's measurable value shows up as
  cheaper verification of the contract that matters (round 4), routing
  toward integration seams and self-consistency defects (rounds 1, 3,
  9), and — on a subject mostly outside its coverage — naming its own
  blind spot so a reviewer knows to read manually instead of trusting
  partial coverage (round 10).
- **Dynamic verification (building and executing the binary against
  hostile inputs) is the strongest single predictor of finding real
  behavioral defects**, a conclusion that has held since round 5 and
  was reinforced through round 9: non-TTY panics, render-loop
  re-parses, scroll/wrap interactions, narrow-pane text truncation, and
  the round 10 findings that could only be shown by constructing and
  running a fixture (fork-PR 403s, an oversized mermaid document
  actually blowing the size cap). None of these have any footprint on
  a signature-level diff or map.
- **The map cannot see three things by design**, each identified by a
  different round: (1) regressions whose mechanism lives in unchanged
  code or a dependency's own semantics (round 5); (2) whether a
  data-flow wire that exists actually carries the *correct* value,
  as opposed to *a* value (rounds 7, 8, 9); (3) anything outside its
  representational model at all — round 10's YAML/shell surface
  rendered as "skipped," not analyzed, and round 17's opposite-
  direction case, a Rust diff fully inside the map's *language*
  coverage but whose entire risk (rendered log text) sits outside its
  *signature* coverage, which the map correctly rendered as silence
  rather than a fabricated marker. Either shape — skipped-as-language
  or silent-as-signature — must trigger the same reviewer response:
  fall back to manual tracing and dynamic verification, not read
  absence of markers as absence of risk.
- **Doc-versus-implementation drift is a defect shape adversarial
  reading plus execution is specifically good at** (dominant in round
  4's dead-guard comment and round 7's four separate instances) —
  and claims of conformance to a past lesson are themselves review
  targets, not review shortcuts (round 7: a doc comment asserted
  "not per frame" while the code recomputed on every idle poll).
- **A first-PR-for-a-feature is a structural blind spot for both
  review strategies** (round 10, sharpened by a post-merge follow-up):
  neither arm's prompt is set up to ask "does this diff's own premise
  hold against the ref it will actually run against, on its own
  introducing PR" — a question that, by definition, only applies to
  the PR that introduces a capability a workflow in the same PR
  builds-and-uses from a base checkout. The orchestrator caught both
  the flag-availability gap and (later, by watching the merged
  workflow actually run) a related checkout-ref bug and an
  action.yaml-doesn't-exist-on-base bootstrap gap, neither of which
  either review arm was positioned to ask about unprompted.
- Orchestrator spot-checks / post-merge verification have repeatedly
  graduated from redundant to load-bearing (round 6: the only
  performance measurement; round 7: seeding a real should-fix; round
  10: catching the bootstrap bug pre-merge and a second, related
  workflow bug post-merge) — the third angle earns its place doing
  what neither review arm's prompt currently does: quantitative
  measurement, distrust of self-reported compliance, and verifying
  a merged workflow's actual first live run.
- **The map's value extends to mechanical compliance checks, not just
  logic-level attention routing** (round 22): a stale ADR status field
  and a file-size warning crossing this repo's own warn threshold
  mid-PR were both caught as pure bookkeeping the report already
  tracks, resolved without any deep code reading — consistent with
  CLAUDE.md's own framing of the file-size warning as "authoritative,"
  not merely advisory.
- **The map can be defeated by its own coverage heuristics being
  wrong, not just incomplete** (round 22, sharpening rounds 7-9's
  "wrong value on a data-flow wire" one level further): a test-only
  helper function outranked real production hotspots in the fan-in
  report because the underlying `is_test_path` classification silently
  misclassified an entire directory convention (`*_tests/`) as
  production code — a defect in what feeds the map, not in how the map
  presents what it's given. No unit test in the PR's own 1121-test
  suite could have caught this, because every test fixture in that
  suite hand-sets its `is_test` flags rather than deriving them from a
  real path through `is_test_path`; only running the actual report
  generator against the repository's own real directory layout
  exercised the gap, reinforcing dynamic verification's role as the
  check no amount of unit testing substitutes for.

## Next

- Consider a map feature flagging cross-crate duplicate-domain
  symbols (round 3 hypothesis, still open).
- Round 5's tool hypothesis (list unchanged symbols that changed
  symbols newly depend on) also remains open.
- Consider an automated check (clippy lint or a draw-path assertion)
  for the "no unbounded recompute inside the render loop" invariant —
  it has now appeared three times (rounds 3, 7, and PR #55's spec
  guarding against it), which is enough recurrence to justify
  mechanizing it.
- Fix the stdin-diff + `--tui` startup failure found in round 8
  (crossterm's input reader fails to initialize because stdin was
  already consumed reading the diff) — a pre-existing gap in a
  documented use case (ADR 0016/0017), not a regression of that PR.
- Round 9's near-miss (a shared private helper causing a display bug
  invisible to a symbol-level map) suggests a possible map feature:
  flag when two or more independent top-level `Report` fields
  (`files`/`tests`/`skipped`/`removed`) feed into the same
  downstream construction function, since that shape recurs whenever
  a `Report` consumer merges several "list of things about a path"
  fields back into one per-path structure.
- Round 10's new failure mode (neither arm checks whether a PR's own
  premise holds against its own base ref) suggests adding an explicit
  "bootstrap consistency" check to the review-prompt template for any
  PR introducing a CLI flag/feature that a CI/Action step in the same
  PR builds-and-uses from a base checkout: does the base ref actually
  have the capability the new workflow step assumes? Neither the map
  nor a diff-focused control read is positioned to ask this
  unprompted, so it likely needs to be an explicit instruction rather
  than something either strategy discovers on its own.
- Round 10 also flags an experiment-design gap worth fixing before the
  next non-Rust-heavy subject: consider giving arm A a lightweight
  non-map "what does the map NOT cover" summary (e.g. the skipped-
  files list alone, without the rest of the report) as a control, to
  isolate whether the attention-routing benefit this round attributes
  to the map specifically requires the full report or would come from
  just knowing the coverage boundary.
- Round 22's `is_test_path` gap (issue #114 — the Rust `LanguageSupport`
  impl doesn't recognize this repo's own `*_tests/` directory
  convention as test code) is itself a candidate future subject: once
  fixed, a follow-up round could confirm the fan-in hotspot list no
  longer surfaces `nav_tests`/`tree_tests`/`order_tests` helper
  functions ahead of real production hotspots.
