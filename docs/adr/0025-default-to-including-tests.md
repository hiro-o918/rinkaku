# 0025. Default to including test symbols in output

- Status: accepted
- Date: 2026-07-13
- Supersedes: parts of [ADR 0009](0009-exclude-test-symbols-from-output-by-default.md)
  (the flag polarity and default; the per-language test-detection heuristics
  and the `## Tests` summary section are unchanged)

## Context

ADR 0009 introduced `--include-tests` as an opt-in flag and made the
default behavior *exclude* test symbols from `## Change graph` and
`## Definitions`, on the assumption that Markdown/JSON output is read
primarily by human reviewers who find test symbols noisy.

Actual usage since [ADR 0015](0015-tui-for-humans-markdown-for-machines.md)
inverted that assumption:

- **Markdown/JSON is now written primarily for LLMs**, not humans. The
  whole point of rinkaku's "outline" (輪郭) framing is to feed a
  condensed API-surface view of a change to an LLM reviewer.
- **Humans read the TUI**, which since #58 renders test files with a
  distinct badge — the noise problem ADR 0009 solved for Markdown does
  not exist in the TUI, because the TUI can distinguish tests visually
  rather than by omission.
- **For an LLM consumer, test symbols are useful signal**, not noise.
  "Which contracts are exercised by which tests, and which tests
  changed alongside which production symbols" is exactly the kind of
  cross-referencing an LLM reviewer needs, and the `## Tests` summary
  (a per-file count) hides that structure. Including test symbols in
  the graph makes those relationships explicit as edges.

The generated-file exclusion (`--include-generated`, ADR 0010/0011) is
justified by a different reason — generated content is genuinely opaque
noise for both audiences — and is unaffected by this change.

## Decision

Invert the CLI flag: rename `--include-tests` to `--exclude-tests` and
flip the default from *exclude tests* to *include tests*.

- Default behavior across every output mode (Markdown, JSON, TUI,
  Mermaid): test symbols appear in `## Change graph` and
  `## Definitions` alongside production symbols, and the `## Tests`
  summary section is omitted (it only ever appeared when tests were
  being excluded).
- `--exclude-tests` restores the previous default: test symbols are
  detected per language, dropped from `## Change graph`/`##
  Definitions`, and summarized as per-file counts under `## Tests`.
  `TagsResolver`'s repo-wide dependency index (ADR 0003) applies the
  same exclusion under this flag, matching ADR 0009's original wiring.

The per-language test-detection heuristics from ADR 0009 (Rust
`#[cfg(test)]`/`#[test]`, Go `*_test.go`, Python `test_*.py`/`*_test.py`
and `tests/` directories, TypeScript `*.test.ts(x)`/`*.spec.ts(x)` and
`__tests__/`) stay in place unchanged — only the flag polarity and
default value flip. `rinkaku-core` keeps its internal `include_tests:
bool` parameter with its original meaning ("true means include tests");
the CLI adapter passes `!cli.exclude_tests` when threading it through.

## Alternatives

- **TUI-only default flip**: leave `--include-tests` as-is for
  Markdown/JSON, but make the TUI include tests by default regardless.
  Rejected: the flag's meaning would become dependent on the output
  mode, which is confusing for scripts that pipe stdout somewhere else
  and for users who move between modes. The complexity of a
  mode-dependent default is not worth the small benefit of preserving
  ADR 0009's Markdown default given the audience shift documented
  above.
- **Keep ADR 0009's default unchanged**: rejected because the audience
  the default was chosen for (human Markdown readers) is no longer the
  primary Markdown audience.
- **Drop test detection entirely**: rejected. `--exclude-tests` is a
  real use case (e.g. an LLM reviewer that only cares about production
  API surface), and the per-language heuristics from ADR 0009 remain
  the right implementation for it.

## Consequences

- **Breaking CLI change**: any script using `--include-tests` breaks
  and must switch to omitting the flag (to get the new default) or to
  `--exclude-tests` (to keep the previous default's exclusion). This
  tool has not shipped a stable CLI yet, so the breakage is acceptable
  pre-1.0.
- Default Markdown/JSON output grows: diffs that touch tests now show
  those test symbols in the graph. This is the intended outcome given
  the LLM-consumer framing above; users who want the old shape opt in
  via `--exclude-tests`.
- The `## Tests` summary section is now opt-in, only appearing under
  `--exclude-tests`. `hotspots` fan-in counting (ADR 0013) already
  operates on whatever symbols are present in the graph, so the fan-in
  ranking naturally now includes test-to-production edges when tests
  are included — this is a genuine signal for an LLM reviewer ("this
  production symbol is exercised by N tests") rather than noise to
  suppress.
- The generated-file default (`--include-generated`) is not changed by
  this ADR — the rationale for excluding generated content applies
  equally to human and LLM consumers.
- README, `--help` text, and any CI invocations of `rinkaku
  --include-tests` must be updated in the same change.
