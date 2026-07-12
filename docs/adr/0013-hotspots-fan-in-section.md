# 0013. Surface fan-in hotspots in rendered output

- Status: accepted
- Date: 2026-07-12

## Context

PR review answers two distinct questions, and this ADR names them
because later ADRs will keep referring back to the split:

- **Question A: "where do I start reading?"** Answered by the
  entry-point tree (ADR 0008), since condensed for readability (ADR
  0012).
- **Question B: "what breaks if I touch this?"** Blast radius / fan-in.
  Currently unanswered by any output.

The tree structurally suppresses fan-in. A shared dependency is
expanded in full the first time it is reached and every later
occurrence collapses to `(see above)` (`render_tree_node`'s `printed:
HashSet<String>` in render.rs); foldable leaf types are additionally
inlined per-parent as `— uses:` annotations (ADR 0012). Both
mechanisms exist to keep the tree short, but their side effect is that
"this struct is referenced from three different call chains" never
appears anywhere as a number — it is scattered across however many
places the node happens to be reached from, most of them collapsed
away.

The data to answer question B already exists and needs no new model.
`SymbolGraph` (graph.rs) carries the full edge list
(`Edge { from, to, is_cycle }`), and render.rs already builds the
forward grouping of those edges keyed by `edge.from`
(`children_by_node`) for tree rendering. Fan-in is the symmetric
reverse grouping, keyed by `edge.to`. `Report.graph` (render.rs)
already serializes the full edge list to JSON, so no new data reaches
the JSON boundary either — only a precomputed aggregation of what is
already there.

## Decision

- Add a pure aggregation in `rinkaku-core` that groups `graph.edges` by
  `to` and produces, per node, a referrer count and the sorted list of
  referrer names. This is arithmetic over existing `Edge` values, not a
  new graph concept — no new node or edge kind, no change to
  `SymbolGraph`.
- **Markdown**: a new `## Hotspots` section, placed between `##
  Change graph` and `## Definitions`. It lists only symbols with
  fan-in >= 2, sorted by fan-in descending, ties broken by path then
  name for determinism. Line format:

  ```
  - struct FooRequest (api/types.go) — used by 3: HandleFoo, HandleBar, SyncFoo
  ```

  The section is omitted entirely when no symbol reaches fan-in >= 2,
  matching the existing precedent of omitting empty sections rather
  than printing them empty.
- **JSON**: expose the same aggregation as a new `hotspots` field
  alongside the existing `graph` field, so machine consumers get the
  ranking precomputed instead of re-deriving it from raw edges. Raw
  edges are unchanged — this is additive.
- Cycle edges (`is_cycle: true`) still count toward fan-in. A cyclic
  reference is still a reference; hiding it would understate blast
  radius exactly where a design smell (ADR 0008's cycle warning) is
  already flagged.

## Alternatives

- **Mermaid flowchart of the symbol graph**: renders fine for a
  handful of nodes but becomes a hairball at 10-15, which is precisely
  the size of PR where blast-radius help matters most. Rejected as
  degrading on the cases it needs to serve.
- **Per-line fan-in annotations on tree rows only**: keeps the signal
  embedded in reading order instead of ranked, so the "widely used"
  property stays buried the same way it is today — only annotated
  instead of absent. Does not solve the actual problem (nothing ranks
  importance).
- **Leave aggregation to JSON consumers** (edges are already present
  in `Report.graph`): Markdown is the primary LLM- and human-facing
  surface; both benefit from a precomputed ranking, and a human reading
  Markdown gets nothing under this alternative since they do not touch
  the JSON output at all.
- **TUI-only fan-in display**: an interactive surface is a separate
  decision (ADR 0015). Static/machine-facing output (`gh pr diff |
  rinkaku` piped into an LLM, JSON consumers) needs blast radius
  regardless of whether an interactive mode ever ships.

## Consequences

- Question B gets a first-class, static answer. Combined with the
  planned symbol-change classification (ADR 0014), fan-in count
  becomes the basis for a risk ranking: "a widely used symbol whose
  contract changed" is exactly the highest-risk case a reviewer wants
  surfaced first.
- Additive change to Markdown (new section only, existing sections
  unchanged) and to JSON (new field, existing `graph.edges` unchanged)
  — unlike ADR 0008/0012, this does not break existing output.
- The fan-in >= 2 threshold is a judgment call, not derived from data.
  Revisit if dogfooding on real diffs shows fan-in == 1 entries carry
  useful signal (e.g. a single caller in a different file than the
  callee, which the tree may still obscure).
- `## Hotspots` reuses the word "hotspot" already used informally for
  the busiest-file summary line (ADR 0012); the two are different
  concepts (file-level symbol density vs. per-symbol fan-in) living in
  the same output, so naming or wording may need a follow-up pass if
  this reads as ambiguous in practice.
