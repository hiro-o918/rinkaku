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

## Amendment (2026-07-14, feat/tighten-file-size-thresholds)

The original thresholds (`WARN_LINE_THRESHOLD: 1500`,
`SPLIT_LINE_THRESHOLD: 2000`) were calibrated against PR #82's
outlier files (3000-4800+ lines). In practice they proved too loose
for rinkaku's primary use case — reviewing LLM-generated diffs, which
tend to grow a single file past a healthy size well before either
number is crossed. The `#[path = "tests.rs"]` test-extraction
convention this ADR's Decision section already establishes (point 8)
means a file over `WARN_LINE_THRESHOLD` on production code alone is
already a responsibility-mixing signal, not just a size one — the old
1500/2000 pair let that signal go unraised for too long.

Change: lower both constants by 500 lines, keeping the 500-line gap
between them:

- `WARN_LINE_THRESHOLD: usize = 1000` (was `1500`)
- `SPLIT_LINE_THRESHOLD: usize = 1500` (was `2000`)

CLAUDE.md's "File size discipline" table is updated to match: normal
≤600, watch 600-1000, warn >1000, split >1500. The `≤600` "normal" and
"600-1000" "watch" bands are descriptive only (no corresponding
consts) and shift down by the same 500 lines, keeping every band's
width unchanged.

A one-tier shift, rather than a more aggressive drop, is a deliberate
compromise against alert fatigue: tightening further would start
flagging files that are legitimately at rest, not drifting, and repeat
warnings a reviewer learns to ignore defeat the whole point of ADR
0028. The existing escape hatch (justify continued growth in the PR
body instead of splitting) is unchanged.

No change to `FileSizeWarning`, `FileSizeSeverity`, or the sort order —
this amendment is a constant-value change only. The always-present
per-file line count/band surface described below is a separate
amendment landing in the same PR, not a consequence of the threshold
change itself.

## Amendment (2026-07-14, feat/tighten-file-size-thresholds, part 2)

Decision 2 above only reports files that already cross
`WARN_LINE_THRESHOLD`. Dogfooding this PR itself surfaced the gap: a
file sitting just under the threshold gives a reviewer no signal at
all, even though "how close is this file to the line" is exactly the
kind of drift-over-time question ADR 0028's Context section is about.

**1. Four-tier classification, always computed.** New
`FileSizeBand { Normal, Watch, Warn, Split }` in `file_size.rs`,
alongside a new `NORMAL_LINE_THRESHOLD: usize = 600` (the boundary
CLAUDE.md's table already described informally but had no constant
for) and `classify_file_size(line_count) -> FileSizeBand`, the single
function both the existing `compute_file_size_warnings` (refactored to
call it) and the new `compute_file_size_bands` build on — one threshold
ladder, not two independently-maintained ones. `compute_file_size_bands`
returns a `FileSizeEntry { path, line_count, band }` for *every* file in
its input, sorted by path ascending (there is no "most attention-worthy
first" concern the way `compute_file_size_warnings` has — every file is
listed, not just the ones worth flagging).

**2. `Report.file_size_bands: Vec<FileSizeEntry>`, additive.** Same
`(path, line_count)` pairs `analyze_diff`/`analyze_repo` already collect
for `file_size_warnings`, so no new IO. `file_size_warnings` and
`FileSizeSeverity` are unchanged and still drive nothing new — kept
because `Badges.file_size_warn_count`/`file_size_split_count` (the
directory-level aggregate badges) intentionally stay Warn/Split-only
(see point 4).

**3. Markdown: `## File size warnings` becomes `## File sizes`.**
Renamed and widened to list every analyzed file, not only Warn/Split
ones — the two sections would otherwise show overlapping information
for exactly the files a reviewer most wants to see (a Warn/Split file
would appear twice under the old scheme: once in the warnings section,
once if a future "always show" section were added alongside it).
Format, one line per file in `file_size_bands`'s order:

```markdown
## File sizes

- `path/to/normal.rs` (80 lines)
- `path/to/watch.rs` (700 lines, watch)
- `path/to/warn.rs` (1200 lines, warn)
- `path/to/split.rs` (2500 lines, split)
```

`Normal` gets no suffix (nothing to flag); every other band appends
`, {band}`. The `⚠`/`🚨` glyphs the old section used are dropped: with
four bands instead of two, a growing zoo of glyphs was worse than a
plain word, and the TUI surface (point 4) already establishes
"text label, no emoji" as this feature's house style. Section placement
(after `## High fan-in symbols`, before `## Definitions`) is unchanged
from the original decision. Skipped entirely when `file_size_bands` is
empty, same "skip when empty" rule as every other optional section.

**4. TUI: the file-row `lines:N` badge is now unconditional.**
`Badges.own_file_size_severity: Option<FileSizeSeverity>` becomes
`own_file_size_band: Option<FileSizeBand>`, populated from
`file_size_bands` (every file) rather than `file_size_warnings`
(Warn/Split only) — so `lines:N` renders on every file row, not only
oversized ones. Color follows the band: `Normal` unstyled, `Watch`
yellow, `Warn`/`Split` red with `Split` additionally bold, so the two
red bands remain visually distinguishable. `Badges.file_size_warn_count`
/ `file_size_split_count` (the directory-level `warn:N split:N`
aggregate) are deliberately **not** widened to count `Watch` files —
those two counts exist to answer "how many files need action", and a
`Watch` file needs none yet; widening them would dilute a signal that
today means "these directories have Warn/Split files in them" into a
vaguer "these directories have some files that are somewhat large."
No emoji, matching the crate's established TUI convention (this ADR's
original decision 6, reaffirmed after PR #104 removed `DIM` from
`DarkGray` text for the same "keep it legible across terminals" reason).

**5. JSON: `file_size_bands` is a new additive top-level field**,
always present (empty array when `files` is empty), mirroring how
`file_size_warnings`/`fan_ins` are always present. `file_size_warnings`
is untouched — this is a pure addition, not a replacement, at the JSON
level (unlike the Markdown section, which does replace the old one).

## Alternatives (amendment)

- **Fold the new field into `file_size_warnings` by adding
  `FileSizeSeverity::Normal`/`Watch` variants** — rejected.
  `FileSizeSeverity` is deliberately the Warn/Split-only type the
  directory aggregate badges (`file_size_warn_count`/
  `file_size_split_count`) and the Markdown warnings label depend on;
  widening it would force every existing match over `FileSizeSeverity`
  to grow two new arms it has no use for, whereas a sibling
  `FileSizeBand` type keeps each concept's match exhaustive over only
  the variants it actually cares about.
- **Keep `## File size warnings` alongside a new, separate "## File
  sizes" section** — rejected as the redundant-information outcome
  point 3 above already argues against: a Warn/Split file would appear
  in both sections with the same line count, just formatted two ways.
- **Show the line count in the Detail pane's per-file view unconditionally
  too** (today: only when `size_warning` is `Some`) — deferred. The
  Detail pane's dedicated warning line answers "why is this file
  flagged", which only makes sense for Warn/Split; showing an
  unconditional "N lines, normal" line there as well was judged noise
  for the common case (most files are Normal) without a concrete
  request driving it. Can follow later if it turns out to matter.

## Consequences (amendment)

- Markdown output for every repository with at least one analyzed file
  now includes a `## File sizes` section (previously only shown when a
  file crossed `WARN_LINE_THRESHOLD`) — a bigger, but purely additive
  (in the "more information, not less accurate" sense), change to the
  default Markdown surface than decision 2's threshold shift alone.
  Downstream Markdown parsers pinning section presence/absence need to
  account for `## File sizes` now appearing on ordinary diffs, not only
  large-file ones.
- JSON gains one new field (`file_size_bands`); no existing field
  changes shape.
- TUI file rows always show `lines:N`; directory rows are unchanged
  (still Warn/Split-only aggregates).
- `docs/experiments/0001-map-assisted-llm-review/rounds/021.md` and
  `022.md` reference the old `## File size warnings` heading as
  historical fact about those specific review rounds — left unedited,
  same "historical record, not rewritten" rule this ADR's own
  Consequences section already applies to itself.
