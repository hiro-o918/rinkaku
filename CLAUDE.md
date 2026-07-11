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
- Run `make test` before committing; it must pass together with
  `make lint`.

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
