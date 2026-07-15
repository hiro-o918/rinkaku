# 0051. mod.rs module-layout convention, enforced by clippy

- Status: Accepted
- Date: 2026-07-15

## Context

Rust offers two equivalent ways to give a module its own submodules:
a same-named file (`foo.rs`) alongside a `foo/` directory holding the
submodules, or a `foo/mod.rs` directory-only layout. This repo had
drifted toward the second style as its de facto majority — `app/`,
`tree/`, `ui/`, `order/`, `review/`, `render/`, and every ADR 0028
test-split tree (`*_tests/mod.rs`) already use directory + `mod.rs` —
but nothing enforced it, and three modules ended up using the other
style instead: rinkaku-tui's `source_diff.rs`/`source_split.rs` and
rinkaku-core's `language.rs`. A mixed convention costs a reviewer a
moment of "which style is this file" every time they open a module
they haven't touched before, for no offsetting benefit.

## Decision

**Standardize on `mod.rs` style** (directory + `mod.rs`, never a
same-named `.rs` file next to its own submodule directory), matching
the repo's existing majority.

**Enforce it with `clippy::self_named_module_files = "warn"`**,
declared once in the workspace `Cargo.toml` and opted into by every
crate via `[lints] workspace = true`. `make lint`'s `-D warnings`
promotes it to a hard error in CI, so the convention cannot silently
drift again.

**Let the lint's own output decide what needs converting**, not a
manual guess: only `rinkaku-core/src/language.rs` actually triggered
it. `language.rs` declares its submodules (`go`/`python`/`rust`/
`typescript`) via ordinary `pub mod x;`, which Rust resolves into a
same-named `language/` directory — the exact ambiguous-resolution
shape the lint targets. `source_diff.rs`/`source_split.rs` only use
`#[path = "source_diff/tests.rs"] mod tests;` to relocate a single
test file, not directory-based submodule resolution, so the lint does
not fire on them and they are left untouched.

`rinkaku-core/src/language.rs` was converted to
`rinkaku-core/src/language/mod.rs` via `git mv` (content unchanged).

## Alternatives

- **Convert `source_diff.rs`/`source_split.rs` too**, on the
  assumption that "same basename as a directory" alone was the
  convention violation. Rejected: they don't hit the lint's actual
  rule (no same-named-directory submodule resolution), and converting
  them would just be a cosmetic rename with no lint backing it —
  exactly the kind of drift this ADR is meant to prevent by relying on
  a checked rule instead of a manual read of the file tree.
- **A `rustfmt`/CI script check** instead of a clippy lint. Rejected:
  `clippy::self_named_module_files` already exists upstream and covers
  exactly this case; no need to hand-roll an equivalent check.

## Consequences

- Every crate's `Cargo.toml` gains `[lints] workspace = true`; the
  workspace `Cargo.toml` gains `[workspace.lints.clippy]` with this
  one entry. Future same-named-module-file introductions fail
  `make lint` immediately instead of drifting unnoticed.
- `rinkaku-core/src/language.rs` moves to
  `rinkaku-core/src/language/mod.rs`; no behavior or public API change.
