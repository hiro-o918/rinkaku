# CLAUDE.md

Project-specific instructions for working on rinkaku.

## Project overview

rinkaku (輪郭, "outline") is a CLI that condenses large PR diffs —
especially LLM-generated ones — into just the **signatures of changed
symbols and their dependencies**, so reviewers and LLMs can grasp the API
surface of a change without reading every implementation line.

- Input: a unified diff via stdin (`gh pr diff 123 | rinkaku`), or
  `rinkaku --base main` (runs `git diff` internally).
- Core: tree-sitter parses changed files, finds the definitions
  containing changed lines, and slices out their signatures.
- Dependency expansion: 1-hop references via tree-sitter tags queries
  (v1). LSP-based resolvers (pyright, gopls, etc.) are pluggable later
  via a `Resolver` trait.
- v1 built-in languages: Rust, Go, Python, TypeScript, each implemented
  as a `LanguageSupport` trait impl (grammar crate + tags query +
  signature-slicing rule), making language support additive.
- Output: Markdown or JSON, designed to be fed to LLMs.

Design rationale for the choices above is recorded in
[`docs/adr/`](docs/adr).

## Architecture principles

- **Flat module structure**: start simple, split into submodules only
  when a group of files sharing a responsibility grows large enough to
  warrant it. Do not pre-create deep module hierarchies.
- **Core logic is pure**: no IO, no wall clock, no environment variable
  reads inside `rinkaku-core`'s domain logic. Diff input, file reads,
  process invocations (`git`, future LSP servers) belong at the boundary
  (`main.rs` and future adapter modules), passed in as arguments or via
  ports.
- **Ports as traits, defined on the consumer side**: `LanguageSupport`
  (tree-sitter grammar + tags query + signature slicing per language) and
  `Resolver` (dependency resolution strategy) are traits defined where
  they are consumed, not where they are implemented. Keep them small
  (1-3 methods).
- **Composition root in `main.rs`**: dependency wiring (which
  `LanguageSupport` impls are registered, which `Resolver` is used) is
  assembled by hand in `main.rs`. No DI framework, no global state,
  no `init()`-time wiring.
- Do not introduce a shared/common abstraction (e.g. a generic repository
  layer over language support or resolvers) speculatively — only after a
  concrete second use case demands it, and after discussing it in an ADR.

## Test strategy

- **rstest + pretty_assertions**, unit tests live in-module
  (`#[cfg(test)] mod tests` at the bottom of the file under test).
- Test/case names follow `should_<behavior>_when_<condition>`.
- **Compare whole structs/values, not individual fields.** Use
  `pretty_assertions::assert_eq!` on the complete expected value. If a
  partial comparison is unavoidable, leave a comment at the top of the
  test explaining why.
- **No mocking of external processes** (git, tree-sitter parses, future
  LSP servers). Extract the pure transformation logic into a function
  that takes plain data in and returns plain data out, and unit-test
  that. Reserve integration-style tests (using real fixtures under
  `resources/` or `tempfile::TempDir`) for the adapter boundary itself.
- **When a test module grows large, split it by topic**, not by
  mechanical top/bottom cut: sibling in-module `mod`s, or
  `#[cfg(test)] #[path = "..."] mod tests;` files under a `*_tests/`
  directory (ADR 0028). Don't let one file's tests grow into a single
  monolithic block. Precedents: PR #93 (`rinkaku-core`) and PR #95
  (`rinkaku-tui`).
- Run `make test` before committing; it must pass together with
  `make lint`.

## Reviewing changes (dogfooding)

When reviewing a branch or PR of this repository (directly or via
review subagents), always cover **three complementary angles** — see
[`docs/experiments/0001-map-assisted-llm-review/`](docs/experiments/0001-map-assisted-llm-review/README.md)
for why:

1. **Map-assisted pass**: generate rinkaku's own output for the diff
   (`rinkaku --base main`, built from a **trusted `main` checkout**,
   never from the branch under review) and use its hotspots, contract
   markers, and entry-point trees to pick deep-reading targets —
   this pass is best at integration seams and architecture-level
   defects.
2. **Independent pass**: a plain review without the map, covering
   line-level correctness and repo conventions (assert style, no
   library-code `unwrap`/`expect`, comment discipline).
3. **Dynamic verification** (mandatory, not left to reviewer
   discretion): build and actually execute the changed surface,
   including failure-mode invocations — non-TTY stdin/stdout, empty
   input, conflicting flags, missing files. Experiment 0001 round 2's
   best finding (a non-TTY panic) came from an *uninstructed* decision
   to run the binary; do not rely on that luck. Either reviewing pass
   may carry this step, but someone must.

The map allocates attention; it is not a verifier. Behavioral bugs do
not show up on the signature surface, so neither pass may skip reading
the code it flags — and none of the three angles may be skipped.

## Toolchain

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --all --check + cargo clippy --all-targets --all-features -- -D warnings
make format  # cargo fmt --all
make help    # list targets
```

CI (`.github/workflows/lint-and-test.yaml` → `wc-rust-test.yaml`) runs the
exact same `cargo` commands as the Makefile — never add a check to CI that
isn't also runnable locally via `make`.

## File size discipline

Files grow by drift, not by design — see ADR 0028 for the rationale.
Keep source files small enough that a reviewer can hold the outline in
one context window.

- **Thresholds** (mirror the `pub const`s in `rinkaku-core/src/file_size.rs`;
  changing them is an ADR amendment):
  - ≤ 1000 lines — normal, no action.
  - 1000–1500 lines — watch; a follow-up split is on the horizon.
  - > 1500 lines — warn; start planning the split now.
  - > 2000 lines — split it, or write down in the PR body why not.
- **Co-locate tests, but move them out when they push the file over.**
  Default to `#[cfg(test)] mod tests { ... }` in the same file. When
  production + test lines together exceed a threshold, switch to
  `#[cfg(test)] #[path = "tests.rs"] mod tests;` so the counted file
  stays under threshold without hiding the tests. Canonical example:
  PR #82's `rinkaku-tui/src/app/{mod,input_key,tests}.rs`.
- **Split along responsibility, not line count.** If a file is over
  threshold, look for an independent responsibility to extract before
  reaching for a mechanical top/bottom cut. Canonical example: PR #82's
  `rinkaku-core/src/render/` split Markdown, Mermaid, and JSON
  rendering into sibling modules because each is an independent output
  format, not because the numbers demanded three parts.
- **rinkaku's own warning is authoritative.** When
  `rinkaku --base main` (from a trusted `main` build, per the
  dogfooding rule above) prints a `## File size warnings` entry for a
  file this PR touches, treat it as a review-blocker hint: either
  perform the split in the same PR, or justify the continued growth
  in the PR body. Do not merge past the warning silently.

## Conventions

- **English only**: code comments, documentation, ADRs, and commit
  messages are all in English (this is an OSS project).
- **Conventional Commits** with English subjects (`feat:`, `fix:`,
  `chore:`, `docs:`, `refactor:`, `test:`, `ci:`, ...). One logical
  change per commit.
- **ADRs are required** for structural decisions: layer/architecture
  choices, adding a major external dependency, introducing a shared
  abstraction, or breaking a public API/output format. Write the ADR in
  `docs/adr/` (MADR-style: Status/Context/Decision/Alternatives/
  Consequences) before implementing the decision.
- Comments explain *why*, not *what* — avoid restating code in prose.
