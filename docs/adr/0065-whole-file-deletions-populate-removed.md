# 0065. Whole-file deletions populate `report.removed`

Date: 2026-07-31

## Status

Accepted

## Context

`analyze_diff` short-circuits a `ChangeKind::Deleted` file into
`report.skipped` (`reason: "deleted"`) before ADR 0014's
`classify_against_base` ever runs, so a diff that deletes a file
*entirely* contributes nothing to `report.removed` — even though every
symbol that file contained is gone from the PR's public contract. A
*partial* deletion (the file survives, one symbol inside it is removed)
already reports correctly. Every consumer of `report.removed` inherits
the gap: the Markdown "Removed symbols" section (ADR 0014), mermaid
`removed` nodes (ADR 0037), and the `--format digest` strikethrough
lines (ADR 0036). The gap predates ADR 0037/0036 and is documented in
both ADRs' own limitations; issue #115 tracks it.

## Decision

When `analyze_diff` meets a `ChangeKind::Deleted` file, it now *also*
reads the base-side content via `read_base_file` (when available),
extracts every symbol with `extract_all_symbols`, and reports each one
as a `RemovedSymbol` — before pushing the existing `SkipReason::Deleted`
entry, which stays untouched. The two records deliberately co-exist:
skipped describes "no head-side content to analyze for `files`/the
graph", removed describes the base-side contract that vanished.

Rules, mirroring the surviving-file pipeline arm one-for-one:

- No `changed_ranges`-style overlap filtering: the whole file is the
  removal, so every top-level base symbol is removed by definition —
  `classify_symbols`' `old_changed_ranges` overlap check has no
  meaningful equivalent here.
- Binary files, `generated_paths` entries, and (unless
  `--include-generated`) base content carrying a generated marker
  (ADR 0010/0011) contribute nothing, exactly as they are excluded from
  analysis while alive.
- Unsupported languages contribute nothing (no parser to extract with).
- `read_base_file` absent or failing leaves `removed` untouched — ADR
  0014's "never guess" contract.
- Test symbols are *not* filtered out, matching the existing behavior
  for partial deletions (`classify_symbols` collects removed test
  symbols too; `--exclude-tests` only partitions `files`).

## Alternatives

- **Synthesize `old_changed_ranges` covering the whole file and reuse
  `classify_against_base`.** Rejected: it would route through
  `classify_symbols`' head-vs-base matching with an empty head side just
  to fall out the other end as "everything removed" — extra moving parts
  for a case whose answer is known up front, and `classify_against_base`'s
  `Added` special case plus range plumbing would need dead-weight
  parameters for the deleted kind.
- **Report the deleted file in `files` with an empty `FileReport`.**
  Rejected: `files` drives the head-side tree/graph, and a file with no
  head-side content has nothing to anchor there; `skipped` +`removed`
  already covers both halves of what a reviewer needs to know.

## Consequences

- `report.removed` (JSON), the Markdown "Removed symbols" section,
  mermaid `removed` nodes, and the digest's `~~name~~ — removed` lines
  now include whole-file deletions whenever a base reader is available
  (`--base`/`--pr` mode). This is an additive output change: no existing
  line/field changes shape, lists only gain entries.
- ADR 0037's and ADR 0036's "whole-file deletions are invisible"
  limitation notes are superseded by this ADR for the base-reader
  modes.
- Stdin mode passes `read_base_file: None` (no known base commit, per
  ADR 0014's own wiring) and keeps today's skipped-only behavior — the
  same "never guess" line every other classification already draws
  there.
