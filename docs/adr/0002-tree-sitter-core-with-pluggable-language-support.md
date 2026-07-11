# 0002. tree-sitter as the core extraction engine, with pluggable language support

- Status: accepted
- Date: 2026-07-11

## Context

rinkaku must locate the definitions containing changed lines in a diff and
slice out their signatures, across multiple languages, running inside CI
on arbitrary repositories. The extraction engine needs to work without
requiring the target project to be built, indexed, or have its
dependencies installed — CI runners are ephemeral and diffs can touch
files whose build is broken or mid-review.

## Decision

Use tree-sitter as the core extraction engine for v1, not an LSP server.
Each supported language is implemented as a `LanguageSupport` trait impl
that bundles: a tree-sitter grammar crate, a tags query (for finding
definitions and references), and language-specific signature-slicing
rules (e.g., where a Rust `fn` signature ends vs. its body). v1 ships
Rust, Go, Python, and TypeScript as built-in `LanguageSupport` impls.

## Alternatives

- **LSP servers (pyright, gopls, rust-analyzer, ...) as the core engine**:
  gives precise, type-aware symbol resolution, but requires the target
  project to build and its dependencies to be installed/indexed first —
  too slow and too fragile for a tool meant to run on arbitrary,
  possibly-broken PR diffs in CI. Reserved for the `Resolver` trait
  (see ADR 0003) as an optional, opt-in upgrade path.
- **Regex/heuristic-based signature extraction**: no external dependency,
  but brittle across languages and doesn't generalize; would need
  per-language special-casing without the structure tree-sitter provides.

## Consequences

- No project build/index step is required; rinkaku can run against any
  checkout, including ones with broken dependencies, and stays fast
  enough for CI.
- Language support is additive: adding a language means adding a new
  `LanguageSupport` impl, not touching the core extraction pipeline.
- Signature slicing accuracy depends on the quality of each language's
  tags query and slicing rules — these need per-language test fixtures.
- Precision is bounded by what syntax alone can tell us (see ADR 0003 for
  how dependency resolution addresses this trade-off).
