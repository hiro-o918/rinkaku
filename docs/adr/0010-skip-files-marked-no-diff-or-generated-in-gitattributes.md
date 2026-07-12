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
(`-diff`) or whose `linguist-generated` is set. Skipped files appear
under the existing `Skipped files` section with a new `generated` skip
reason. Without a repository, no attribute filtering happens (best
effort). A `--include-generated` flag restores the previous behavior,
mirroring `--include-tests`.

## Alternatives

- **Parse `.gitattributes` ourselves**: avoids the `git` dependency for
  the pure-stdin case, but attribute resolution (macros, per-directory
  files, precedence) is subtle and `git check-attr` is authoritative;
  rinkaku already shells out to `git` in its main modes.
- **Filter by filename heuristics (lockfile lists etc.)**: duplicates
  what repositories already declare, and drifts per ecosystem.

## Consequences

- Generated/lockfile churn disappears from the output while staying
  visible as skip entries; repositories control the behavior through
  their own `.gitattributes`.
- JSON consumers see a new `skipped.reason` value (`generated`) — a
  minor breaking change for strict enum consumers, acceptable pre-1.0.
- Pure-stdin-outside-a-repo input keeps generated files; documented
  limitation rather than a fragile homegrown attribute parser.
