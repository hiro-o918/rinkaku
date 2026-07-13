# 0039. Revise `--format mermaid`'s visual encoding and legend

- Status: accepted
- Date: 2026-07-14

## Context

ADR 0021 chose `classDef added`/`changed`/`fan-in` (red fill, heavier
stroke) and ADR 0037 added `removed` (gray fill, dashed stroke). ADR
0036 added a one-line English legend composed by
`compose_and_post_comment.sh`, next to the rendered diagram:

```
_Legend: green = added · orange = API changed · gray dashed = removed · red heavy border = fan-in_
```

User feedback on this pair, after using it on real PR comments:

1. The legend line itself is hard to parse at a glance — four
   `color/attribute = meaning` clauses packed into one sentence, read
   without ever seeing the corresponding node styles next to it.
2. "Removed" reading as gray-dashed is not the intuitive color for
   deletion; red is the color most reviewers already associate with
   "this is gone," and pairing it with a dashed border (already used
   elsewhere in this format for "not a normal solid thing" — the cycle
   edge, ADR 0021) would read as more self-explanatory.
3. `fan-in`'s existing encoding (red fill, heavier stroke) collides
   with the color reviewers now expect for "removed" once (2) takes
   red. Fan-in needs its own encoding that doesn't compete for the same
   color.

This ADR revises the `removed` and `fan-in` `classDef`s and replaces
the one-line legend with a self-describing `Legend` subgraph rendered
inside the mermaid diagram itself, styled with the real `classDef`s a
reader is about to see used for real nodes.

## Decision

### Color/style reassignment

- **`added`**: unchanged — green fill (`#c6f6d5`), solid border.
- **`changed`** (API/signature changed): unchanged — orange fill
  (`#feebc8`), solid border.
- **`removed`**: reassigned to **red**, dashed border — muted red fill
  (`#fed7d7` — the same fill ADR 0021 originally used for `fan-in`,
  now free since `fan-in` moves off red) with a red stroke
  (`#9b2c2c`) and `stroke-dasharray: 5 5` (unchanged dash pattern).
  Red communicates "gone" on first read without relying on the legend;
  dashed keeps the "not a normal solid node" vocabulary this format
  already established for the cycle edge (`-.->`) and, until now,
  `removed` alone.
- **`fan-in`**: moved off red entirely to avoid colliding with the new
  `removed` encoding, onto **a distinct blue/violet stroke plus a
  label suffix**: fill `#e9d8fd` (light violet), stroke `#553c9a`
  (deep violet), `stroke-width:3px` (kept — the heavier border is
  still part of "pay attention to this node," just no longer red).
  Additionally, a fan-in node's **label gains a `(in:N)` suffix**
  (`n0["shared (in:3)"]`) where `N` is `fan_ins[i].used_by.len()` —
  encoding the signal in text as well as color/stroke, not color
  alone. `render_mermaid` already has `report.fan_ins` in scope (used
  today only to build `fan_in_ids` for class lookup); this reads
  `used_by.len()` from the same collection.

  This double-encoding (color *and* text) is deliberate: a reviewer who
  can't rely on color (colorblindness, a grayscale print of the PR
  page) still gets "this is a hotspot, referenced by 3 others" from
  the label text alone, the same way `render_markdown`'s `## Fan-in`
  section already spells the count out in prose. It also removes the
  need for a reader to distinguish "heavier red border" from "removed"
  by stroke weight alone under a quick glance — the label makes the
  distinction textual, not just visual.

  Precedence when a node is both `changed`/`added` and high-fan-in is
  unchanged from ADR 0021: `fan-in` styling (now violet + label
  suffix) wins, for the same reason ADR 0021 gave (blast radius is the
  more decision-relevant fact for a glance-level view).

### Legend: a `subgraph Legend` inside the diagram, not a prose line

Replace `compose_and_post_comment.sh`'s `MERMAID_LEGEND` prose line
with a small `subgraph Legend` block appended to `render_mermaid`'s own
output (and `render_mermaid_file_level`'s, for consistency — the
fallback path uses the same four `classDef`s), placed after every real
subgraph/edge/class-assignment line but before the trailing
`classDef` declarations:

```
  subgraph Legend
    legend_added["added"]
    legend_changed["API changed"]
    legend_removed["removed"]
    legend_fan_in["fan-in (in:N)"]
  end
  class legend_added added
  class legend_changed changed
  class legend_removed removed
  class legend_fan_in fan-in
```

- **Always emitted**, including on the empty-graph path and the
  file-level fallback — a reader who only ever sees a small or
  aggregated graph still gets the same self-describing key, and a
  reader who never opens the `<details>` prose still has it inline.
- **Real `classDef`s, not prose describing them**: a legend built from
  the same four classes actually used on the graph's real nodes cannot
  drift out of sync with a future color/style change the way a
  hand-written English sentence already has (this ADR exists because
  the sentence and the styles disagreed on what "removed" should look
  like). Changing a `classDef` automatically re-styles the legend
  entries too, in the same commit, by construction.
- **`compose_and_post_comment.sh`'s `MERMAID_LEGEND` line is deleted.**
  The legend now lives inside the fenced ```` ```mermaid ```` block
  rendered by `rinkaku-core`, not composed by the comment-assembly
  script — the same "legend is part of the diagram, not commentary
  about it" reasoning ADR 0036 used in reverse (there, the legend was
  explicitly kept *out* of `render_mermaid` because it was "prose about
  this comment's presentation, not data derived from the `Report`");
  here the legend is no longer prose, it is graph content (real nodes
  with real classes), which is exactly the kind of thing
  `render_mermaid` already owns.
- **Node ids**: `legend_added`/`legend_changed`/`legend_removed`/
  `legend_fan_in` are fixed, hand-written ids (not sequential `n{i}`)
  since the legend is not derived from `report.graph.nodes` — using the
  `n{i}` sequence could theoretically collide with a real node's id if
  the legend were appended mid-sequence; fixed, clearly-namespaced ids
  sidestep that entirely and make the legend block greppable/stable
  across renders.
- **No edges in the Legend subgraph.** The legend's job is "what does
  this color/style mean," not "what talks to what" — adding connecting
  edges between legend entries would imply a call relationship that
  does not exist and dilutes the one clear reading (four independent
  swatches).

## Alternatives

- **Keep the one-line prose legend, just fix the color/dash wording.**
  Rejected: doesn't address complaint (1) — a corrected sentence is
  still four clauses a reader has to hold in their head and cross
  reference against the diagram above it, rather than seeing the exact
  same rendered style directly.
- **Fan-in encoded by stroke-width alone (thicker border), no color
  change, no label suffix.** Considered as a smaller change. Rejected:
  a heavier border on the *same* red fill as `removed` is exactly the
  collision complaint (3) describes — a reviewer distinguishing
  "removed" from "high fan-in" by stroke width alone, at a glance, in a
  PR comment's small viewport, is not a reliable signal. Moving off
  red entirely (not just adjusting weight) is what actually removes
  the collision.
- **Fan-in label suffix only, no color change (keep red fill).**
  Rejected for the same collision reason as above — the label suffix
  alone helps distinguish once a reader is already looking closely, but
  the *first* glance still reads "red = ambiguous between removed and
  fan-in" before the label is read. Both the color move and the label
  suffix are needed; the ADR keeps both.
- **Drop the in-diagram legend subgraph; keep prose but shorten it to
  a table.** A Markdown table renders fine inside the PR comment body
  but not inside the ```` ```mermaid ```` fence itself, so it would
  still live in `compose_and_post_comment.sh`, inheriting the same
  "can drift out of sync with the real `classDef`s" risk this ADR is
  trying to close. Rejected in favor of the self-describing subgraph.
- **Legend subgraph with emoji/icons per class instead of text
  labels.** Rejected per this repository's established
  no-decorative-glyphs convention (ADR 0028's terminal-rendering
  rationale, reapplied here) — plain text names of the classes
  themselves, consistent with the (now-deleted) prose legend's own
  wording style.

## Consequences

- `MERMAID_CLASS_DEFS` changes are a breaking change to this format's
  *rendered colors* (not its structure): existing consumers reading
  `--format mermaid` output as text (not just rendering it) that
  pattern-match on the literal hex values would need updating. No
  external consumer is known to do this (the format's only shipped
  consumer is the GitHub Action added alongside ADR 0021), so no
  compatibility path is added.
- Every `render_mermaid`/`render_mermaid_file_level` test's expected
  output grows the trailing `Legend` subgraph block — a mechanical,
  full-string update per this repository's fully-qualified assert
  convention, not a partial-comparison exception.
- `compose_and_post_comment.sh` no longer composes `MERMAID_LEGEND`;
  the mermaid section becomes exactly the fenced diagram, nothing
  appended after it. The oversized-mermaid fallback branch (mermaid
  content past `MAX_MERMAID_LENGTH`) no longer has a legend to show
  either, since there is no diagram at all in that case — consistent
  with the legend being part of the diagram, not independent prose.
- The `(in:N)` label suffix is unique to fan-in-classed nodes; `added`/
  `changed`/`removed` node labels are unchanged (name only, per ADR
  0021/0037's "coarse and readable" rule) — this ADR does not extend
  the suffix convention to any other class.
- If a future class is added to this format, its legend entry follows
  the same fixed-id, no-edges pattern established here.
