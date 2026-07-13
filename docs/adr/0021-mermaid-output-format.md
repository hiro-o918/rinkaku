# 0021. `--format mermaid`: an opt-in, GitHub-native graph output

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0013 rejected a "mermaid flowchart of the symbol graph" as a
Markdown addition: it "renders fine for a handful of nodes but becomes
a hairball at 10-15, which is precisely the size of PR where blast-
radius help matters most." ADR 0015 went further and drew a hard line:
Markdown/JSON are for machine consumers and stay stable; all future
human-facing visual work routes to the TUI, explicitly ruling out
"mermaid diagrams... in Markdown."

Both rejections are about mermaid *inside the default Markdown output*
— an addition every existing consumer (LLMs piping `rinkaku`'s stdout,
CI diffing the text) would receive whether they wanted it or not, and
sized however large the change graph happens to be.

This ADR is a different shape of request: a reviewer skimming a PR on
github.com, in the one place that already renders mermaid natively
inline — a PR comment or the PR description. Nothing today gives that
audience a call/dependency graph at a glance; the TUI (ADR 0015) is
terminal-only and unavailable to someone just reading the PR page. The
audience is new (PR-page reviewers, not stdout/LLM consumers or
terminal users), the surface is opt-in (`--format mermaid`, mutually
exclusive with `--format md`/`--format json` the same way those already
are with each other), and it is meant to be produced by a bot (the
GitHub Action added alongside this ADR) and posted as its own comment,
not blended into the Markdown a human or LLM already reads.

The hairball objection is still real and is addressed head-on below,
not argued away.

## Decision

Add `OutputFormat::Mermaid` and `render_mermaid(&Report) ->
Result<String, RenderError>` to `rinkaku-core`, plus a `--format
mermaid` CLI value. This does not touch `render_markdown` or the JSON
path at all — every existing consumer's output is byte-for-byte
unchanged.

- **Direction: `flowchart LR`.** A call/dependency chain reads
  caller-to-callee left-to-right the same way `render_markdown`'s tree
  reads top-to-bottom (parent-then-child); `LR` keeps that same mental
  model instead of introducing `TD`'s top-down convention for what is
  the same underlying edge direction the tree already uses.
- **Nodes: name only.** No kind prefix (`fn`/`struct`/...), no
  signature — the design goal (from the user) is coarse and readable,
  legible at a glance, not a second copy of "Definitions". A reviewer
  wanting the signature already has the Markdown/TUI output; this
  format answers "what talks to what," nothing more.
- **Grouping: one `subgraph` per file**, titled with the path — this is
  what lets a glance answer "how many files, how concentrated" the same
  way `change_graph_summary`'s "most in ..." line does for Markdown,
  but visually instead of as a sentence.
- **Styling via `classDef`**: `added` (green-tinted), `changed`
  (orange-tinted), `hotspot` (red-tinted, heavier stroke; renamed to
  `fan-in` by ADR 0034). A node that is both `changed` and a hotspot
  gets the `hotspot` class — precedence documented at the call site in
  `render.rs` — since "this is a wide blast radius" is the more
  actionable fact for a glance-level view than "this signature
  changed," and the node's own subgraph/label already shows it changed
  via the Definitions/Markdown companion output. Colors are chosen with
  explicit dark-on-light text so they hold up
  under both GitHub's light and dark themes (mermaid's own theming
  otherwise only flips background, not a fixed text color).
- **Cycle edges** render as `-.->` (dashed) instead of `-->`, mirroring
  the ⚠️ warning line `render_markdown` already gives a cycle edge —
  visually distinct without needing a label.
- **Hairball fallback: a node budget.** When `graph.nodes.len()` exceeds
  a constant (30 — large enough that a real single-PR change graph
  almost always fits under it per dogfooding so far, small enough that
  a flowchart at that size is still readable in a PR comment's
  viewport), rendering falls back to a **file-level graph**: one node
  per file (no subgraphs, nothing to nest), edges aggregated between
  files and deduplicated with a count label (`-- 3 -->`), changed files
  (any file containing an `Added`/`SignatureChanged` symbol) styled via
  the same `changed` class. This directly answers ADR 0013's objection:
  past the size where a symbol-level flowchart degrades into a
  hairball, the format demotes itself to the granularity mermaid can
  still render legibly, rather than rendering the hairball anyway. A
  leading `%% aggregated to file level (N symbols > budget)` comment
  marks that the fallback fired, so a reader isn't left wondering why
  symbol names disappeared.
- **Determinism**: node/edge order follows the existing `graph.nodes`/
  `graph.edges` vec order (already diff-derived and stable, per
  `graph.rs`'s doc comments); aggregated file-level edges dedupe
  first-seen-order, same convention `render_markdown` already uses
  elsewhere (e.g. `change_graph_summary`'s path tie-break).
- **Empty graph**: still emit a minimal valid document (`flowchart LR`
  plus a `%% no symbols` comment) rather than an empty string or a
  panic — unlike `render_markdown` (which returns `""` for a fully
  empty report), a mermaid code fence with no `flowchart` header is not
  valid mermaid, and the GitHub Action always wants *something* it can
  post.
- **Label escaping**: node ids are regenerated sequentially (`n0`,
  `n1`, ...) rather than reusing `NodeId` strings — mermaid node ids
  cannot contain many characters a path or symbol name might (`.`,
  `/`, `::`, `@`). Labels are quoted (`n0["name"]`) with embedded `"`
  escaped as `&quot;` and `[`/`]` escaped the same way, since an
  unescaped bracket would prematurely close the label.

## Alternatives

- **Add mermaid to default Markdown output**: exactly what ADR 0013 and
  ADR 0015 rejected, for the reasons restated in Context. Not
  reconsidered here — this decision does not touch that output at all.
- **Render every node individually regardless of graph size**: true to
  "just show the graph," but reproduces ADR 0013's hairball at the
  exact size where the format would matter most for a real PR.
  Rejected in favor of the file-level fallback.
- **Fallback: truncate to the top-N hotspot nodes instead of
  aggregating to file level**: keeps symbol-level detail but silently
  drops nodes, which misrepresents the graph's shape (missing edges to
  the dropped nodes look like "nothing depends on this" rather than
  "not shown"). Aggregating to file level keeps every symbol
  represented, just at coarser granularity, and says so explicitly via
  the leading comment.
- **`flowchart TD`**: considered for consistency with mermaid's
  "flowchart" example default, but a wide, shallow call graph (a common
  shape — see `change_graph_summary`'s multi-root case) reads
  awkwardly top-down in a narrow PR-comment viewport; `LR` fits a wide
  shallow tree more naturally in that context.

## Consequences

- A new format value joins `md`/`json`, each mutually exclusive with
  the others and with `--tui` — no change to that exclusivity
  machinery beyond adding the third value.
- The node budget (30) is a judgment call, not derived from data;
  revisit if dogfooding a real PR's mermaid output shows the threshold
  reads as a hairball before it's reached, or feels overly conservative
  after it.
- This format is additive and consumed primarily by the GitHub Action
  (see the accompanying `action.yaml`/workflow), not by
  `gh pr diff | rinkaku` piped into an LLM — that path stays on
  Markdown/JSON per ADR 0015.
- File-level aggregation means a very large diff's mermaid output
  cannot be pivoted back to symbol level from this format alone; a
  reader who wants that detail follows the Markdown/TUI companion
  output the Action also posts (in a collapsed `<details>` section).
