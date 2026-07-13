# 0040. Move the mermaid legend back out to a CI-generated Markdown block

- Status: accepted
- Date: 2026-07-14

## Context

ADR 0039 moved the `--format mermaid` legend from a one-line prose
sentence composed by `compose_and_post_comment.sh` into a
`subgraph Legend` block rendered inside the diagram itself by
`rinkaku-core`, specifically to avoid the risk of the legend's wording
drifting out of sync with the real `classDef` styles.

User feedback on real PR comments after that change: the `Legend`
subgraph makes the diagram itself noticeably larger and harder to read
— four extra nodes plus their `class` assignments compete for the same
small viewport as the actual call/dependency graph, which is the part
of the diagram a reviewer actually came to read. The complaint ADR 0039
was fixing (a prose legend that could say the wrong thing) and the
complaint this ADR fixes (an in-diagram legend that crowds out the
diagram) pull in opposite directions, so neither "leave it in the
diagram" nor "go back to hand-written prose" resolves both.

The drift risk ADR 0039 identified is real but was never fundamentally
about *where* the legend text lives — it was about the legend being
**hand-written** and therefore able to disagree with the `classDef`
values a maintainer changes later. That risk is addressed just as well
by generating the legend text mechanically from the same source of
truth (the `.mmd` file's own `classDef` lines), regardless of whether
the result is rendered inside or outside the mermaid fence.

## Decision

- **Remove `MERMAID_LEGEND` and the `subgraph Legend` block** from
  `rinkaku-core`'s `render_mermaid`/`render_mermaid_file_level`. Both
  functions keep emitting `MERMAID_CLASS_DEFS` (the `classDef
  added/changed/fan-in/removed` lines) as before —
  `write_legend_and_class_defs` is renamed to `write_class_defs` and
  only writes those.
- **`compose_and_post_comment.sh` generates a Markdown Legend section**
  after the ```` ```mermaid ```` fence, by parsing the `classDef` lines
  out of the `.mmd` file it already reads (`MERMAID_PATH`) rather than
  hand-typing hex values or prose:
  - Extract each `classDef <name> fill:#RRGGBB,...` line's class name
    and `fill` color with a regex/`sed` pass.
  - Map class name → a short English description via a small
    associative array the script owns (`added` → "new symbol", etc.)
    — the *meaning* of each class still has to be written by a human
    somewhere, same as ADR 0039's node labels did; only the *color*
    is derived mechanically now.
  - Render one Markdown table row per parsed `classDef`, using GitHub's
    inline math rendering to show an actual colored swatch:
    `` $`{\color{#c6f6d5}\blacksquare}`$ `` — this renders as a filled
    square in the fill color, giving the same "see the real color, not
    a word describing it" property the in-diagram legend had, without
    adding nodes to the diagram itself.
  - A `classDef` name with no entry in the script's description map is
    skipped with a warning (`::warning::`) rather than failing the
    step — a future class added to `rinkaku-core` without a matching
    script update degrades to "legend has one fewer row" instead of
    breaking comment posting.
  - If no `classDef` lines can be parsed at all (empty/missing mermaid
    section), the Legend block is omitted entirely, consistent with
    how the mermaid section itself is already omitted when there is no
    diagram.
- This still satisfies ADR 0039's original drift concern: the legend's
  *colors* are read from the same `.mmd` output the diagram itself
  used, in the same CI run, so they cannot independently go stale the
  way the ADR-0039-era hand-written prose sentence could. The only
  hand-maintained part left is the description text, same as before
  this ADR and the same as ADR 0039's node labels.

## Alternatives

- **Keep the in-diagram `subgraph Legend` (ADR 0039 as-is).** Rejected:
  does not address the concrete complaint (diagram size/readability);
  this ADR exists because that complaint arrived after ADR 0039 shipped.
- **Revert fully to ADR 0039's rejected one-line prose sentence,
  hand-maintained.** Rejected: reintroduces the exact drift risk ADR
  0039 was written to close. The regex-parse approach gets the
  "outside the diagram" property back without giving up "derived from
  the real styles."
- **Have `rinkaku-core` emit the Markdown legend as a separate
  `--format` output (e.g. alongside `--format digest`), instead of
  having the shell script parse `classDef` lines out of the `.mmd`
  text.** Considered — would keep the color→meaning mapping inside
  Rust instead of bash. Rejected for this change: it adds a fifth
  output format and a new action.yaml probe/wiring step for a single
  consumer (this one script), which is disproportionate to the
  problem. Parsing the already-produced `.mmd` text is a few lines of
  `sed`/regex against a format (`classDef name fill:#hex,...;`) that
  `rinkaku-core` already treats as a stable literal (ADR 0039's own
  Consequences section already calls a `classDef` hex-value change a
  breaking change to the format). Revisit as a real Rust-side output
  if a second consumer of the same mapping shows up.
- **Hardcode the four hex values directly in
  `compose_and_post_comment.sh` instead of parsing them out of the
  `.mmd` file.** Rejected: this is exactly the hand-maintained,
  driftable duplication both this ADR and ADR 0039 are trying to
  avoid — a `classDef` color change in `mermaid.rs` would silently
  stop matching a hardcoded copy in the shell script.

## Consequences

- `rinkaku-core`'s `--format mermaid` output shrinks back down by the
  four legend nodes/classes ADR 0039 added; every `render_mermaid`/
  `render_mermaid_file_level` test's expected output loses the
  `Legend` subgraph block (mechanical, full-string update, per this
  repository's fully-qualified assert convention).
- The rendered PR comment gains a Markdown Legend section between the
  mermaid fence and the digest `<details>`, generated by
  `compose_and_post_comment.sh` from the `.mmd` file's `classDef`
  lines rather than typed by hand.
- The oversized-mermaid fallback (mermaid content past
  `MAX_MERMAID_LENGTH`) has no `classDef` lines to parse (there is no
  diagram at all in that case), so it has no Legend section either —
  same behavior as ADR 0039's in-diagram legend had for that path.
- `docs/adr/0039-mermaid-visual-encoding-revision.md`'s "Legend: a
  `subgraph Legend` inside the diagram" decision is superseded by this
  ADR; ADR 0039's color/style reassignment (added=green, changed=
  orange, removed=red-dashed, fan-in=violet+label-suffix) is
  unaffected and remains in effect.
