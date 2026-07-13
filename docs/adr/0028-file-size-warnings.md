# 0028. File size warnings as a first-class attention dimension in rinkaku's output

- Status: accepted
- Date: 2026-07-13

## Context

Today's session started with a symbol-follow scroll bug in the TUI
(PR #80) and ended with a 12000-line mechanical refactor (PR #82) that
split three files — `render.rs` (4837 lines), `ui.rs` (4154 lines), and
`app.rs` (3475 lines) — into module directories. None of those files
was designed to be that large. Each grew by 100–300 lines per PR over
many merges, with no single PR ever presenting a "this file is now
huge" moment for a reviewer to react to. By the time the size became
obvious, the split was much more expensive than an incremental split
along the way would have been.

This is the standard mode of failure for oversized files in a codebase
this size: they don't grow by design, they grow by drift. A code-review
process that only looks at "what changed in this PR" cannot catch a
file that grew from 3000 to 3300 lines in a single acceptable diff.
Rinkaku's whole reason to exist is to allocate reviewer attention
across signals a plain diff hides — this is exactly one of those
signals.

Related prior art: ADR 0013 already introduced `## Hotspots` for a
different attention dimension (fan-in / blast-radius). The section
header framing in ADR 0013 — "here is a list of nodes that deserve
extra scrutiny, computed once and shown when non-empty" — is exactly
the shape file-size warnings need.

Related self-critique: PR #82 had to happen at all because rinkaku's
own output was silent about the three files it was about to reshape.
The tool telling *itself* to split its own source is the direct
motivation for this ADR.

## Decision

**1. Add a distinct `FileSizeWarning` type to `Report`.** New module
`rinkaku-core/src/file_size.rs`:

```rust
pub struct FileSizeWarning {
    pub path: String,
    pub line_count: usize,
    pub severity: FileSizeSeverity,
}

pub enum FileSizeSeverity { Warn, Split }
```

Both derive `Serialize` so the JSON output is additive. Stored on
`Report` as a new field `pub file_size_warnings: Vec<FileSizeWarning>`.

**2. Two thresholds, empirical from PR #82.** As `pub const` on the
same module:

- `WARN_LINE_THRESHOLD: usize = 1500` — over this, the file is
  reported as `FileSizeSeverity::Warn` ("start planning the split")
- `SPLIT_LINE_THRESHOLD: usize = 2000` — over this,
  `FileSizeSeverity::Split` ("this needs to be split, or the PR body
  must justify the growth")

A file at exactly 1500 lines is not warned (the check is strictly
greater); a file at exactly 2000 lines is `Warn`. The rationale for
these particular numbers is the observed pain point: at ~1500 lines a
reviewer can still hold the file's outline in one context window, at
~2000 they cannot, at ≥3000 (the PR #82 files) any change requires a
grep-first workflow that itself hides drift.

Changing these constants is a spec change and must be an ADR amendment
— consumers (LLM reviewers, human review policy, potential future CI
integration) rely on them as stable.

**3. Line counting happens inside `pipeline.rs`.** `analyze_diff` and
`analyze_repo` already call the `read_file` port and hold each
changed/scanned file's content in scope during tree-sitter parse.
Count lines (`content.lines().count()`) there, collect `(path,
line_count)` pairs, then call the pure
`compute_file_size_warnings(&pairs) -> Vec<FileSizeWarning>` right
after `compute_hotspots(&graph)`. Skipped files (binary, generated,
deleted, unsupported-language) are not measured — they either have no
content or are outside rinkaku's concern.

**4. Markdown surface.** New `## File size warnings` section, placed
immediately after `## Hotspots` and before `## Definitions` — both are
attention allocators, and a reviewer scanning the output top-to-bottom
should see the fan-in and file-size dimensions consecutively rather
than one below the definitions body. Skipped entirely when the vec is
empty (mirrors ADR 0013's "Hotspots is skipped when empty" rule).
Format:

```markdown
## File size warnings

- ⚠ `path/to/file.rs` (1734 lines) — over the 1500-line watch threshold; consider splitting
- 🚨 `path/to/big.rs` (4837 lines) — over the 2000-line split threshold
```

The `⚠` / `🚨` glyphs match ADR 0014's marker convention (single
high-contrast glyph, colored consistently). Ordering: `Split` before
`Warn`, then within each severity, `line_count DESC` then `path ASC`
for stability.

**5. JSON surface.** `file_size_warnings` becomes a top-level field on
the `Report` JSON, always present (empty array when nothing warns),
mirroring how `hotspots` is always present. `FileSizeSeverity`
serializes as `"warn"` / `"split"` (`#[serde(rename_all =
"snake_case")]`).

**6. TUI surface (three touch points, no new pane).**

The TUI deliberately conveys severity through **text labels + color**
(no emoji glyphs) everywhere it surfaces file-size warnings. Terminal
emoji rendering width is inconsistent enough to distort the Tree pane's
column layout — and once the Tree pane had to drop the glyph, the
Status line and Detail pane were switched over too so the reviewer
never has to reconcile two different legends for the same signal. The
Markdown/JSON output keeps its `⚠` / `🚨` glyphs (rendered outside a
terminal, no width problem).

- **Status line** (`ui/status.rs`): when
  `report.file_size_warnings` is non-empty, append
  `"warn:N split:M file-size"` to the status line so the reviewer sees
  the per-severity totals at a glance from any pane. Either half is
  dropped when its count is zero (so an all-Warn report reads
  `"warn:N file-size"`, mirroring how the Tree badge omits a zero
  half).
- **Detail pane on a file row** (`build_file_detail` +
  `ui/detail_pane.rs`): when the cursor lands on a file row whose path
  matches a warning, show one line
  `"Warn: 1734 lines — consider splitting (>1500 watch)"` (yellow) or
  `"Split: 4837 lines — over the 2000-line split threshold"` (red) in
  the Detail pane, positioned above the file's own symbol listing (same
  layout position `top_fan_in` uses on `DirDetail`). Whole-line severity
  color (`Color::Yellow` / `Color::Red`) is applied at the caller so the
  formatter itself stays a pure `String`.
- **Tree pane badge** (`row_view::push_badge_spans`): file rows
  carrying a warning render `lines:N` after the row label — the `lines:`
  prefix stays uncolored, the numeric `N` picks up the severity color
  (yellow for Warn, red for Split), so the eye lands on the number.
  Directory rows aggregate their descendants as `warn:N split:N`, each
  half's numeric portion colored by its own severity; a half whose
  count is zero is omitted so a small subtree never gains a stray
  `split:0`. This mirrors the existing `^N` fan-in badge aggregation
  pattern (`Badges.fan_in`), lets the reviewer see "which files are
  oversized" directly from the Tree browse without opening the Detail
  pane, and — because severity is conveyed by color rather than a glyph
  — reads correctly across every terminal that supports 8+ colors.
- **No new pane, no new keybinding.** Reviewers already have Enter →
  Detail. Adding a dedicated File-size pane would violate ADR 0020's
  one-pane-per-attention-dimension ratio for a feature that fits
  entirely into an existing pane's slot.

**7. Mermaid: no rendering.** Mermaid (ADR 0021) is a graph view of
symbol edges. File size is not a graph property (it belongs to a
node's file, not its incoming/outgoing edges). Rendering file-size on
Mermaid would either require faking an edge (misleading) or bolting a
sidebar into a diagram format that has no place for one. Explicit
non-goal.

**8. Tests are not deducted from the line count.** rinkaku counts
whatever content `read_file` returned, no test-block heuristic. The
CLAUDE.md guideline separately tells authors to use `#[cfg(test)]
#[path = "tests.rs"] mod tests;` (the PR #82 `app/` pattern) when
test weight is what tipped a file over — that keeps the counted file
naturally under threshold, so the tool and the guideline reinforce
each other without either silently working around the other.

## Alternatives

- **Widen `Hotspot` into an enum with a `FileSize` variant** —
  rejected. `Hotspot` today is fan-in-specific (`id: NodeId`, `used_by:
  Vec<String>`); every Mermaid class-assignment (`hotspot_ids`
  collection, `render/mermaid.rs`) and TUI `Badges.fan_in` sum
  (`tree.rs`) reads that shape directly. An enum widening forces every
  consumer to variant-match forever and breaks the existing JSON
  schema. ADR 0013 itself notes the "hotspot" word is informally
  overloaded already; solidifying `Hotspot` as fan-in-only and
  introducing `FileSizeWarning` as a sibling is cleaner than merging
  two concepts under one name.
- **Enforce thresholds in CI** (fail the build over `SPLIT_LINE_THRESHOLD`)
  — rejected here, deferred to `.github/workflows/`. Rinkaku's job is
  attention allocation; enforcement is separate policy. Doing both in
  the same PR would tangle the map-vs-verifier split ADR 0015 and
  0026 both maintain.
- **CLI flag `--file-size-warn=<N> --file-size-split=<N>`** — deferred.
  The two constants ARE the spec; if a downstream repo needs different
  numbers they can fork the tool or submit a follow-up ADR.
  Configurability now would freeze the numbers as "one choice among
  many", weakening the ADR-driven norm.
- **Silently exclude the `mod tests { ... }` block from the count** —
  rejected on principle: it turns the metric into
  "production-code-only lines" which every reviewer would then have
  to mentally reconcile against `wc -l`. The CLAUDE.md guideline
  route is transparent (the author physically moves tests to
  `tests.rs`) and works with any external tool, not just rinkaku.
- **Report only the worst offender** (top 1 file, top 3 files) —
  rejected. `## Hotspots` already reports every fan-in ≥ 2 rather
  than "top N", and this ADR mirrors that "show every threshold
  crossing" rule for consistency.

## Consequences

- Every `Report` fixture in existing tests grows a `file_size_warnings:
  vec![]` line. This is mechanical, but touches a lot of test files —
  the PR should call this out explicitly so a reviewer isn't surprised
  by the diff volume.
- Markdown and JSON output on any repository with a file ≥ 1500 lines
  gains a new section / field. Downstream consumers (LLM reviewers
  parsing the Markdown, machine consumers parsing the JSON) that pin
  the schema will need to acknowledge one new field.
- Mermaid output is unchanged. Any file-size dimension a Mermaid
  viewer wants must come from the JSON alongside.
- rinkaku on its own repo will start warning about
  `rinkaku-core/src/render/markdown.rs` (3627 lines post-refactor —
  already over `SPLIT_LINE_THRESHOLD`) at the first run. This is
  correct: the PR #82 split lowered the top-level files, but the
  Markdown submodule itself is still oversized and marked for follow-up.
- Bumping the constants is an ADR amendment (this file's own decision
  rule), so the numbers cannot silently drift the way the source
  files themselves did.
- CI enforcement, CLI configurability, and a dedicated Mermaid /
  Change-graph annotation for file size are all out of scope; each
  can arrive as its own ADR when there is concrete demand.
