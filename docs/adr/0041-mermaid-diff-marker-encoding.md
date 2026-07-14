# 0041. Diff-style marker prefixes and a `referenced` class for unchanged dependencies

- Status: accepted
- Date: 2026-07-14

## Context

`--format mermaid`'s color/style vocabulary (ADR 0021, revised by ADR
0039, legend moved out by ADR 0040) covers `added`, `changed`,
`fan-in`, and `removed` nodes. A node in the graph that is none of
these — a symbol pulled in only because a changed symbol depends on
it, itself untouched by the diff — gets no `class` assignment at all
today, so mermaid renders it with the theme's default node color.

User feedback: that default-colored node reads as "undefined," not
"unchanged" — there is no `classDef` a reader can look up for it, so
the diagram doesn't actually explain what that color means (unlike
every other node, which now has a Legend entry per ADR 0040). Separately,
color alone is a weak signal for "what changed" even for the classes
that do have one: a reviewer scanning a diagram in grayscale, or
just skimming quickly, benefits from the same textual reinforcement
ADR 0039 already gave `fan-in` (its `(in:N)` label suffix) — but
`added`/`changed`/`removed` currently rely on color and file-level
prose (the Markdown/JSON "Definitions" section) alone.

## Decision

### `referenced` class for unchanged dependency nodes

Add a fifth `classDef referenced` covering any node that is
in the graph but has none of `added`/`changed`/`fan-in`/`removed`
today — i.e. every node ends up with exactly one class, none left
unstyled. Neutral gray, distinct from the other four colors:

```
classDef referenced fill:#e2e8f0,stroke:#4a5568,color:#1a202c;
```

Applied in `render_mermaid`'s existing class-assignment loop as the
`else` arm of the fan-in/added/changed match (previously "no class"),
and symmetrically in `render_mermaid_file_level`'s per-path loop as
the `else` arm of changed/removed-only (previously "no class").
`render_mermaid_file_level`'s path labels get a `classDef` (`changed`/
`removed`/`referenced`) the same way, but never a marker prefix: a
path node aggregates every symbol in that file, so a single `+`/`~`/
`-` character can't represent a file that may contain an added symbol
next to an untouched one.

### Diff-style marker prefix on node labels

Prepend a marker character to a node's label based on the same
classification that drives its `classDef`, ahead of the existing
name and `(in:N)` fan-in suffix:

| Class | Marker | Example label |
| --- | --- | --- |
| `added` | `+ ` | `n0["+ foo"]` |
| `changed` | `~ ` | `n0["~ bar"]` |
| `removed` | `- ` | `n0["- old_helper"]` |
| `fan-in` | *(none — unaffected)* | `n0["shared (in:3)"]` |
| `referenced` | *(none)* | `n0["baz"]` |

`fan-in` gets no marker of its own: per ADR 0039, `fan-in` styling
already wins over `added`/`changed` for a node that is both (a new or
changed symbol also referenced by 2+ other changed symbols), and that
precedence is unchanged here — the node keeps its own
added/changed/removed status implicitly (visible in the Markdown/JSON
Definitions section), while the diagram highlights the fan-in signal
specifically, same reasoning ADR 0039 gave for why fan-in's color wins.
Adding a `+`/`~` marker on top of `(in:N)` would double-encode the
same axis (blast radius vs. contract status) into one label and crowd
it; the two are kept as separate signals: color/stroke-width for
fan-in, marker text for contract status, and only one is shown at a
time on any given node.

Marker characters:

- `+`/`-` are the universal unified-diff convention (`git diff`,
  `diff -u`) for added/removed lines — reusing them needs no
  legend lookup for a reader already familiar with diffs, which
  every user of a diff-summarizing tool is.
- `~` for "changed" rather than a third diff-alphabet character
  (unified diff has no single-char "modified" marker) because `~`
  already reads as "approximately/changed" in common prose and code
  review shorthand, and is visually distinct from `+`/`-` at a glance
  (unlike, say, `*` which is easy to misread as an added-with-emphasis
  marker).
- No marker for `referenced`/`fan-in`: a marker's job here is to flag
  "this row is part of the diff's contract change," so its absence is
  itself informative — "nothing to see here, unchanged" — rather than
  needing its own dedicated glyph.

### Legend and escaping interaction

- `compose_and_post_comment.sh`'s `legend_description()` gains a
  `referenced` case arm, and the existing `added`/`changed`/`removed`
  descriptions are updated to lead with the same marker character, so
  the Legend table doubles as the marker's own key:
  `+ added — new symbol`, `~ API changed — signature changed`,
  `- removed (dashed border in graph)`,
  `referenced — unchanged dependency (not part of the diff)`.
- The marker is prepended to the label text *before*
  `escape_mermaid_label` runs, not after: `+`, `~`, and `-` are not
  among the characters that function escapes (`&`, `"`, `[`, `]`,
  newline), so this ordering is behaviorally identical to appending
  after, but keeps "build the full label, then escape it once" as the
  single escaping contract the function already documents, rather
  than adding a second, marker-specific escape path.

## Alternatives

- **Encode "unchanged dependency" via node opacity/no-fill instead of
  a new `classDef`.** Rejected: mermaid `classDef` doesn't have a
  reliable "no fill, inherit theme" declarative option that stays
  legible under both GitHub's light and dark theme (the same
  constraint ADR 0021 already worked around by hardcoding
  `color:#1a202c` on every class) — an explicit neutral gray is more
  predictable than relying on the reader's active theme.
- **Marker suffix instead of prefix** (`foo +` instead of `+ foo`).
  Rejected: unified diff's own convention is a leading marker column;
  a trailing marker is easy to miss once fan-in's `(in:N)` suffix is
  also present, and a reader scanning top-to-bottom sees the marker
  before the name either way only if it leads.
- **Distinct marker for `fan-in` too** (e.g. `* foo (in:3)`).
  Rejected per the Decision section above — would double-encode two
  independent signals (contract-change status vs. blast radius) on
  the same node without adding information the color/stroke-width and
  `(in:N)` suffix don't already carry, and crowds the label.
- **Skip the `referenced` classDef, leave unclassified nodes
  styleless.** Rejected: this is the status quo this ADR fixes — a
  styleless node still reads as "undefined" rather than "confirmed
  unchanged," and breaks ADR 0040's "every visible style has a Legend
  row" property.

## Consequences

- Every graph node now carries exactly one `class` assignment
  (previously, an unclassified non-fan-in node with no
  added/changed/removed status had none) — a structural invariant a
  future class addition should preserve.
- `render_mermaid`/`render_mermaid_file_level` test expected strings
  gain the `referenced` class on previously-unclassed nodes/paths and
  the `+`/`~`/`-` marker prefixes on added/changed/removed node
  labels — mechanical, full-string updates per this repository's
  fully-qualified assert convention.
- `compose_and_post_comment.sh`'s Legend table grows a fifth row and
  the wording of four existing rows changes (marker prefix); the
  underlying `classDef`-line parsing logic (ADR 0040) is unchanged —
  only `legend_description()`'s hand-maintained text changes.
- No effect on `--format md`/`--format json`/`--format digest` output
  — this is a `--format mermaid`-only visual encoding, verified by
  comparing that format's output on this PR's own diff against a
  `main` build.
