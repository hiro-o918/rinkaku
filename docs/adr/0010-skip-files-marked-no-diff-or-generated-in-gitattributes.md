# 0010. Skip files marked no-diff or generated in .gitattributes

- Status: accepted
- Date: 2026-07-12

## Context

Repositories already declare "not worth diff-reviewing" files in
`.gitattributes`: the `-diff` attribute (git renders them as binary) and
`linguist-generated` (GitHub collapses them in PR views). `gh pr diff`
still emits full hunks for `linguist-generated` files, so lockfiles and
generated code flow into rinkaku's output as noise. This complements the
test-symbol exclusion (ADR 0009): both remove content reviewers have
already declared uninteresting.

## Decision

When a local git repository is available (the `--base` and `--pr` modes,
or stdin piped inside a repo), resolve attributes via `git check-attr`
at the process boundary and skip files whose `diff` attribute is unset
(`-diff`) or whose `linguist-generated` is set, recording a new
`generated` skip reason on each. These entries are always recorded in
`Report.skipped` and therefore always present in JSON output, but are
omitted from the Markdown "Skipped files" section entirely — a
`.gitattributes`-generated file is content the repository has already
declared uninteresting to diff-review, so listing it as something
rinkaku "didn't look at" in output meant for a human/LLM skimming a
change would just be noise on top of what `.gitattributes` already
communicates. Without a repository, no attribute filtering happens
(best effort). A `--include-generated` flag restores the previous
behavior (no filtering at all, in either output format), mirroring
`--include-tests`.

## Alternatives

- **Parse `.gitattributes` ourselves**: avoids the `git` dependency for
  the pure-stdin case, but attribute resolution (macros, per-directory
  files, precedence) is subtle and `git check-attr` is authoritative;
  rinkaku already shells out to `git` in its main modes.
- **Filter by filename heuristics (lockfile lists etc.)**: duplicates
  what repositories already declare, and drifts per ecosystem.

## Consequences

- Generated/lockfile churn disappears from Markdown output entirely;
  repositories control the behavior through their own `.gitattributes`.
- JSON consumers see a new `skipped.reason` value (`generated`) — a
  minor breaking change for strict enum consumers, acceptable pre-1.0.
- Markdown readers cannot tell, from the rendered output alone, that a
  generated file was skipped — unlike every other skip reason
  (`binary`/`deleted`/`unsupported_language`), which still appear under
  "Skipped files". This is a deliberate asymmetry: a reviewer only needs
  to know a generated file existed if they specifically want to check,
  which JSON output (`--format json`, always includes every `generated`
  entry) supports; the common case is that generated-file noise should
  simply not compete for attention in the primary, human-facing rendering.
- Pure-stdin-outside-a-repo input keeps generated files; documented
  limitation rather than a fragile homegrown attribute parser.
- The same `.gitattributes` exclusion also applies to `TagsResolver`'s
  repo-wide dependency index (ADR 0003), not just the diff's own skip
  list: by default it skips every tracked path `.gitattributes` marks
  generated, resolved once for the whole index via a stdin-batched
  `git check-attr --stdin -z` (rather than the diff-scoped, argv-based
  call used for `Report.skipped`, since the index covers every
  `git ls-files` path and could exceed `ARG_MAX` as CLI arguments), so a
  changed production symbol's "Depends on:" cannot resolve to a
  definition living in a generated file (e.g. an ORM's model struct,
  dragging in every column/tag as noise). `--include-generated` restores
  the previous full-index behavior for this too.
