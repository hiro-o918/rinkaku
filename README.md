# rinkaku

> rinkaku (輪郭, "outline" in Japanese)

A CLI that condenses large PR diffs — especially LLM-generated ones — into
just the **signatures of changed symbols and their dependencies**, so
reviewers and LLMs can grasp the API surface of a change without reading
every implementation line.

## What it is

- **Input**: a unified diff via stdin (`gh pr diff 123 | rinkaku`), or
  `rinkaku --base main` to run `git diff` internally. In `--base` mode,
  file contents are read via `git show <head>:<path>` so extraction always
  matches the diffed commit, regardless of the working tree's state. In
  stdin mode, file contents are read off the working tree, which assumes
  **the piped diff is consistent with the current working tree** (e.g. it
  was just produced by `git diff` or already applied); a stale or
  unrelated diff piped via stdin can produce misaligned line numbers.
- **Core**: tree-sitter parses the changed files, finds the definitions
  that contain changed lines, and slices out their signatures.
- **Dependency expansion**: each changed symbol is expanded one hop out to
  the definitions it references, via tree-sitter tags queries (v1).
  LSP-based resolvers (pyright, gopls, etc.) are a pluggable extension
  point (`Resolver` trait) planned for a later release.
- **Languages (v1, built-in)**: Rust, Go, Python, TypeScript. Each is a
  `LanguageSupport` trait implementation (grammar crate + tags query +
  signature-slicing rule), so language support is additive.
- **Output**: Markdown or JSON, designed to be fed to an LLM or read by a
  human reviewer.

See [`docs/adr/`](docs/adr) for the reasoning behind these choices.

## Status

Early development. Diff parsing, tree-sitter extraction, the CLI
(stdin/`--base` input, Markdown/JSON output), and 1-hop dependency
expansion (`--deps`, the tags-based `Resolver`) are implemented. Not
published to crates.io.

## Installation

TBD — not yet published. Once released, this section will cover
`cargo install rinkaku` and prebuilt binaries.

## Usage

Planned CLI shape (subject to change as implementation lands):

```sh
# From a GitHub PR
gh pr diff 123 | rinkaku

# From a local git diff against a base branch
rinkaku --base main

# JSON output for feeding into another tool or LLM
rinkaku --base main --format json
```

## Development

Requires a Rust toolchain (pinned in [`rust-toolchain.toml`](rust-toolchain.toml);
`rustup` will install it automatically).

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + cargo clippy --all-targets --all-features -- -D warnings
make format  # cargo fmt --all
make help    # list available targets
```

CI runs the same `make` targets on every pull request (see
[`.github/workflows/`](.github/workflows)).

### Architecture

Lightweight ports & adapters: core extraction logic in `rinkaku-core` is
pure (no IO, no clock, no env), with tree-sitter parsing and future
LSP/process boundaries isolated behind traits (`LanguageSupport`,
`Resolver`) defined on the consumer side. See [`CLAUDE.md`](CLAUDE.md) and
[`docs/adr/`](docs/adr) for details.

### Roadmap / not yet done

- LSP-backed `Resolver` implementations (pyright, gopls, rust-analyzer,
  ...) as a higher-precision, opt-in alternative to the v1 tags-based
  `Resolver`.
- Release automation (release-please, cross-compiled binary publishing)
  — intentionally deferred out of the bootstrap PR; tracked as a
  follow-up.

## License

MIT
