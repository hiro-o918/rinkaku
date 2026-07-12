# 0017. Whole-repo outline as the default input mode

- Status: proposed
- Date: 2026-07-12

## Context

Every existing input mode (stdin pipe, `--base`, `--pr`; ADR 0004)
requires a diff, so rinkaku can only describe a *change*. Dogfooding
the TUI (ADR 0015) surfaced a second use case: the outline of the
whole repository — all symbols and their dependency structure — for
onboarding and architecture review, with no diff involved. Running
bare `rinkaku` in a terminal today exits with an error demanding
input, which wastes the most natural invocation of the tool on a
failure message.

Two mechanisms were considered for producing a whole-repo report.
A synthetic diff against git's empty tree reuses the existing
pipeline unchanged, but routes the entire repository through a
diff-shaped detour: every file's full content is rendered into a
unified diff only to be re-parsed, every symbol is (mis)classified as
`Added` (ADR 0014's markers become uniform noise), and the changed-
line machinery (`changed_ranges`, innermost-definition suppression)
runs with inputs that make it meaningless. Alternatively, the
extraction layer already has `extract_all_symbols` — the function
`TagsResolver` uses to index the repository — which yields all
definitions directly from file contents.

## Decision

Add a whole-repo mode that builds the report directly from file
contents via `extract_all_symbols`, bypassing the diff pipeline: a
pure core function assembles `FileReport`s (classification left
unknown, as stdin mode already does when no base is available) and
feeds the existing graph/render/TUI layers. The boundary supplies the
file list from `git ls-files`, applying the same test/generated-file
exclusions (ADRs 0009–0011). The 1-hop dependency resolver is skipped
— every symbol is already in scope. This mode is the default when no
input is given: stdin is a TTY and neither `--base` nor `--pr` is
passed. Bare `rinkaku` on an interactive terminal opens the TUI
(ADR 0015: humans get the TUI); an explicit `--format` or a
non-TTY stdout still produces Markdown/JSON.

## Alternatives

- **Empty-tree synthetic diff**: minimal wiring, but pays diff
  generation/parse cost for the whole repository, marks every symbol
  `Added` (semantically wrong — nothing changed), and exercises
  change-oriented code paths whose semantics don't apply. Rejected:
  the mode's meaning is "no diff", so it should not manufacture one.
- **Explicit flag only (`--all`), no default change**: safer for
  compatibility, but bare `rinkaku` keeps erroring on the tool's most
  discoverable invocation. Rejected: the TTY check cleanly separates
  the new default from every existing piped/flagged invocation, so
  nothing that works today changes behavior.

## Consequences

- Bare `rinkaku` becomes useful: repository outline in the TUI, no
  arguments needed. All existing invocations are unaffected (they all
  either pipe stdin or pass a flag).
- Fan-in hotspots (ADR 0013) and cycle explanations now describe the
  whole codebase — the architecture-review use case — but the
  hotspot threshold (fan-in ≥ 2) was tuned for PR-sized graphs and
  will over-fire at repo scale; revisit the threshold or switch to a
  top-N cut if the section proves noisy.
- Name-match edge collection (`collect_edges`) scales with symbol
  count times name-collision rate; common names may inflate edges on
  large repositories. Acceptable for v1; a revisit trigger is TUI
  startup latency becoming perceptible on real repositories.
- The TUI's diff pane (`d`) has nothing to show in this mode and
  renders its placeholder — acceptable until a dedicated whole-repo
  detail (e.g. full signature listing) replaces it.
