# 0009. Exclude test symbols from output by default

- Status: superseded by [ADR 0025](0025-default-to-including-tests.md)
  (flag polarity and default flipped; per-language detection heuristics
  and the `## Tests` summary section — now opt-in — are unchanged)
- Date: 2026-07-12

## Context

Dogfooding the entry-point tree rendering (ADR 0008) on a real branch of
this repository produced 98 graph roots, most of them `should_...` test
functions. Test symbols drown out the implementation symbols the output
exists to surface — for both human reviewers and LLM consumers — and this
will hold for any diff that follows the common practice of changing tests
alongside code. The tool is also meant to support architecture-level
review (module boundaries, separation of responsibilities), where test
code is noise, while the *fact* that tests changed is still a signal a
reviewer wants ("did this change come with tests?").

## Decision

Detect test symbols per language at extraction time and exclude them from
the change graph and definitions by default. Detection lives in each
`LanguageSupport` implementation: path conventions (`tests/` dirs,
`_test.go`, `test_*.py` / `*_test.py`, `*.test.ts` / `*.spec.ts` /
`__tests__/`) plus AST context where paths are insufficient (Rust
`#[cfg(test)]` modules and `#[test]` functions, which are colocated with
production code). Excluded symbols are summarized as per-file counts in a
`## Tests` section so their existence stays visible. A `--include-tests`
flag restores the previous behavior.

The same exclusion applies to `TagsResolver`'s repo-wide dependency index
(ADR 0003), not just the diff's own graph: by default it skips whole
test-path files and drops AST-detected test symbols the same way, so a
changed production symbol's "Depends on:" cannot resolve to a same-named
test helper or fixture elsewhere in the repo. `--include-tests` restores
the previous full-index behavior for this too.

## Alternatives

- **Keep including test symbols**: the status quo; makes real-world
  output unreadable, as observed.
- **Collapse instead of exclude (render names without expansion)**:
  still yields dozens of tree lines per file; counts convey the same
  signal in one line.
- **Path-only heuristics without AST context**: simpler and language-
  agnostic, but misses Rust's in-file `#[cfg(test)]` modules — the
  dominant case in this very repository — so per-language detection via
  `LanguageSupport` is required anyway.

## Consequences

- Default output shrinks to implementation entry points and their
  subgraph; the `## Tests` summary preserves the "tests were changed"
  signal at one line per file.
- Test-detection heuristics can misclassify unconventional layouts;
  `--include-tests` is the escape hatch, and heuristics can be refined
  per language without an output-format change.
- Another breaking change to default output on top of ADR 0008;
  acceptable pre-1.0 and before announcing the format.
- Groundwork for architecture-review features (module-level graph views)
  that assume the graph contains production symbols only.
- Dependency resolution (ADR 0003) is more precise as a side effect: since
  `TagsResolver`'s index excludes test symbols by the same default, a
  production symbol's "Depends on:" no longer surfaces coincidental
  name-matches against test helpers/fixtures — a class of false positive
  the name-only resolver was otherwise prone to.
