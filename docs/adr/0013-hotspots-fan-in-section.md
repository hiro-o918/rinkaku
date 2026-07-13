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

## Amendment (2026-07-13, feat/file-size-warnings)

The tree pane badge originally rendered `~N` for changed symbols and `^N`
for fan-in aggregate (ADR 0015/0016). During work on ADR 0028 (file-size
warnings, which added `lines:N` and `warn:N split:M` badges alongside),
the terse single-glyph prefixes proved illegible to first-time
reviewers: `~` and `^` conveyed no semantic hint by themselves.

Change: the tree pane badges are relabeled as `chg:N` (changed symbols)
and `ref:N` (references, i.e. fan-in aggregate) — text prefixes matching
the file-size badges' `lines:` / `warn:` / `split:` convention. Number
coloring is unchanged; only the label prefix is replaced. `Badges`
struct field names (`Badges.fan_in`, etc.) are left as-is — the change
is purely presentational.

This is a breaking change to the TUI's badge presentation, but has never
shipped a release (per ADR 0015/0016), so no compatibility path is
needed. Consumers of `Report` (Markdown / JSON / Mermaid) are
untouched.

## Amendment (2026-07-13, fix/hotspots-deterministic-tiebreak)

Found while verifying byte-level output determinism during ADR 0031's
parallel-parse work: `compute_hotspots`'s ordering was not fully
deterministic even on `main` alone, unrelated to parallelization. The
existing tie-break (fan-in descending, then `path` then `name` ascending,
per this ADR's Decision section) leaves ties unresolved when two distinct
symbols share both `path` and `name` and reach equal fan-in — e.g. two
overloaded functions named `helper` in the same file, disambiguated only
by the `@{start_line}` suffix in their `id` (`collect_nodes`'s
disambiguation scheme). In that case the order between them was decided
by `referrers_by_target`'s `HashMap` iteration order, which varies
between runs under Rust's randomized `HashMap` hasher seed.

Change: add `id` as a fourth, final tie-break key
(`fan-in desc, path asc, name asc, id asc`) in `compute_hotspots`'s
`sort_by` (`rinkaku-core/src/graph.rs`). `id` is always unique per node
(`collect_nodes`'s contract), so this fully determines the order in every
case. Purely an internal ordering fix — `Hotspot`'s fields and the
Markdown/JSON output shapes are unchanged.

## Amendment (2026-07-13, feat/label-contract-changes-badge)

The 2026-07-13 amendment above deliberately left `!N` (contract-change
count) as a bare glyph, scoping the `chg:`/`ref:` relabeling to only the
two badges that shipped alongside ADR 0028's file-size warnings. User
testing on the resulting tree row (`chg:2 !1 ref:3`) showed this
prediction was wrong: `!` reads as generic "warning/attention" with no
hint of *what* changed, unlike `~`/`^` which at least visually echoed
the diff-marker glyphs (`~` signature-changed) they were replaced for
being illegible — `!` was never even that. The asymmetry of a labeled
`chg:`/`ref:` pair next to one unlabeled glyph also made the row read as
inconsistent on its own.

Change: the contract-change badge is relabeled `api:N`, following the
same split-span pattern as the other text-label badges (only the
numeric N is colored, the label stays default). `api` was chosen over a
literal `contract:` truncation because it is shorter (keeping the
tree column compact, the same concern that motivated `chg:`/`ref:`
over spelling out "changed"/"references" in full) while still legible
on first read — `Badges.contract_changes` counts symbols whose change
altered their public signature/API surface, which rinkaku's own project
description (CLAUDE.md's "grasp the API surface of a change") already
treats as synonymous with "contract" (ADR 0014's own language, "that
inclusion *is* the contract"). The `!` glyph itself is dropped; `api:N`
replaces it entirely rather than prefixing it, matching how `chg:`/
`ref:` fully replaced `~`/`^` rather than combining with them.
`Badges.contract_changes` (the field name) is unchanged — as with the
prior amendment, this is purely presentational.

Unlike `chg:N`/`ref:N` (cyan, informational counts), `api:N`'s number is
rendered in yellow — the same warning color the file-size `warn:` badge
uses — so the color, not just the label, restores the "pay attention"
signal the original `!` glyph carried on its own.

## Amendment (2026-07-13, feat/rename-hotspots-to-fan-in)

Superseded by [ADR 0033](0033-rename-hotspots-vocabulary-to-fan-in.md):
user testing found "hotspot" collides with an unrelated, well-known
term (CodeScene's change-frequency × complexity metric) and the `ref:`
badge label collides visually with the `gr` keybinding despite naming
unrelated concepts. Every "hotspot" identifier and string named in this
ADR (`Hotspot`, `compute_hotspots`, `Report.hotspots`, the Markdown
`## Hotspots` heading, the Mermaid `hotspot` class, and the `ref:N`
badge label) is renamed to "fan-in" by ADR 0033. This ADR's Decision
and both prior amendments remain the historical record of what shipped
and why; see ADR 0033 for the current naming.
