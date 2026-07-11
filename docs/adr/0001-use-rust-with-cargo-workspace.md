# 0001. Use Rust with a Cargo workspace

- Status: accepted
- Date: 2026-07-11

## Context

rinkaku needs a language and project layout for its core engine. The two
main candidates were Rust and Go, both common choices for cross-platform
CLI tooling with good static-binary distribution stories. The core engine
depends on tree-sitter for parsing, and a later milestone plans an
LSP-based `Resolver` (pyright, gopls, etc.) for more precise dependency
resolution.

## Decision

Use Rust, organized as a Cargo workspace with a `rinkaku-core` library
crate (plus a thin `rinkaku` binary) so future bindings or editor
integrations can depend on the core without pulling in the CLI.

Key reasons:

- tree-sitter is a C library. Its official grammars are published as Rust
  crates and statically linked via `build.rs`, giving a pure-Rust build.
  In Go, the equivalent requires CGO, which complicates and slows down
  cross-compilation for the release matrix we need
  (linux/macos x86_64/aarch64).
- The rust-analyzer ecosystem publishes `lsp-types`, a maintained, typed
  crate for the LSP protocol — directly useful for the planned
  LSP-backed `Resolver`. Go's LSP protocol types live inside `gopls`
  internals; third-party standalone libraries exist but are stale.

## Alternatives

- **Go**: faster compile times and a simpler toolchain, but CGO is
  required for tree-sitter bindings, which undermines easy
  cross-compilation — the deciding factor against it.
- **Single binary crate (no workspace)**: simpler to start, but blocks
  future bindings/editor-integration crates from reusing the core engine
  without vendoring the CLI's `clap` dependency tree.

## Consequences

- Cross-compiled release binaries can be built without CGO toolchains.
- `rinkaku-core` stays free of CLI concerns (`clap`, stdout formatting),
  so it can be embedded in other tools (editor plugins, CI bots) later.
- Contributors need a Rust toolchain (pinned via `rust-toolchain.toml`);
  Go was the only alternative considered, and it loses on the tree-sitter
  cross-compilation trade-off above.
