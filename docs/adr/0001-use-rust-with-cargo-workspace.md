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

## Amendment (2026-07-12): split the bin crate out of `rinkaku-core`

The original decision above already named the target shape ("a
`rinkaku-core` library crate (plus a thin `rinkaku` binary)"), but the
bootstrap PR implemented `main.rs` and the `[[bin]]` target directly inside
`rinkaku-core`'s `Cargo.toml` to keep the initial workspace minimal.

This amendment carries out that deferred split: `rinkaku-core` becomes a
lib-only crate (no `[[bin]]`), and a new `rinkaku` crate holds `main.rs`
plus its CLI-only dependencies (`clap`, `log`, `env_logger`), depending on
`rinkaku-core` via a `path` + `version` dependency.

Reason: `cargo install rinkaku` requires a crate literally named `rinkaku`
that produces a `[[bin]]`. A lib crate with an equally-named embedded
binary (`rinkaku-core` producing a `rinkaku` binary) cannot be installed by
crate name — `cargo install` resolves the crate name, not the bin name,
against crates.io. Splitting the bin out is a prerequisite for publishing
to crates.io at all, not an optional cleanup.

Consequences:

- `cargo install rinkaku` now works once both crates are published
  (`rinkaku-core` first, then `rinkaku`, since the latter depends on the
  former by version).
- The `rinkaku` crate's `Cargo.toml` pins `rinkaku-core = { path = ...,
  version = "0.1.0" }`: the `path` is used for workspace-local builds, the
  `version` is what crates.io publishing requires and what a consumer
  installing from the registry resolves against.
- No behavior change: `main.rs`'s composition-root role, and its tests,
  moved unchanged into the new crate.
