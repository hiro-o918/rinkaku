# rinkaku

> rinkaku (輪郭, "outline" in Japanese)

A CLI that condenses large PR diffs — especially LLM-generated ones — into
just the **signatures of changed symbols and their dependencies**, so
reviewers and LLMs can grasp the API surface of a change without reading
every implementation line.

Large diffs (especially LLM-generated ones) bury the handful of lines that
actually matter to a reviewer under implementation detail. rinkaku parses
the changed files with tree-sitter, finds the definitions containing the
changed lines, and renders just their signatures plus one hop of
dependencies — a structural outline instead of the full diff.

## Status

Early development. Diff parsing, tree-sitter extraction, the CLI
(stdin/`--base`/`--pr` input, Markdown/JSON/mermaid output), 1-hop
dependency expansion, and the interactive TUI are implemented.

## Installation

### Homebrew

```sh
brew install hiro-o918/tap/rinkaku
```

### Install script

```sh
curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh | bash
```

You can also specify a version or install directory:

```sh
curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh | VERSION=v0.1.0 bash
curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh | INSTALL_DIR=~/.local/bin bash
```

### From source (cargo install)

```sh
cargo install rinkaku
```

### GitHub Releases (manual)

Download the tarball for your platform from the
[latest release](https://github.com/hiro-o918/rinkaku/releases/latest),
extract it, and place the `rinkaku` binary on your `PATH`:

```sh
tar xzf rinkaku-<target>.tar.gz
mv rinkaku-<target>/rinkaku /usr/local/bin/
```

Where `<target>` is one of `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, or
`aarch64-apple-darwin`.

### Updating

```sh
rinkaku self-update
```

Downloads and installs the latest GitHub release for your platform,
replacing the running binary in place. If you installed via Homebrew or
`cargo install`, prefer `brew upgrade` or `cargo install rinkaku` instead
so your package manager's bookkeeping stays in sync — `self-update` works
either way, but it bypasses those managers.

By default this prompts for confirmation before installing. Pass `--yes`
(or `-y`) to skip the prompt and proceed non-interactively:

```sh
rinkaku self-update --yes
```

When stdin is not a terminal (e.g. run from a script or CI) and `--yes` is
not given, `self-update` refuses to run rather than silently proceeding.

## Quick start

```sh
# Run inside a repository with no arguments: opens the interactive TUI
# with a whole-repo outline (or prints Markdown if stdout isn't a
# terminal)
rinkaku

# Condense a GitHub PR's diff
gh pr diff 123 | rinkaku

# Condense a local diff against a base branch
rinkaku --base main
```

See [CLI usage and output format](docs/cli-usage.md) for every input mode
(stdin/`--base`/`--pr`), output format (Markdown/JSON/mermaid), and flag.

## Key features

- **Signature-first output**: tree-sitter parses the changed files, finds
  the definitions containing changed lines, and slices out their
  signatures — not the full implementation.
- **1-hop dependency expansion**: each changed symbol is expanded one hop
  out to the definitions it references, so a reviewer sees what a changed
  function calls without reading the whole call chain.
- **Four built-in languages**: Rust, Go, Python, TypeScript, each
  implemented as an additive `LanguageSupport` trait implementation.
- **Three output formats**: Markdown and JSON for LLMs/CI, and an opt-in
  mermaid call/dependency graph for pasting into a GitHub PR
  comment/description where it renders natively.
- **Interactive TUI**: a terminal UI for human reviewers, with a
  topologically-ordered entry tree, diff/detail/pivot panes, and
  contract-change markers (added/removed/signature-changed).
- **GitHub Action**: a composite action that posts rinkaku's report as a
  sticky PR comment.

## Documentation

- [CLI usage and output format](docs/cli-usage.md) — every input mode,
  output format (Markdown/JSON/mermaid), and flag
  (`--deps`/`--include-tests`/`--include-generated`/`--entry`), with
  real captured examples.
- [The interactive TUI](docs/tui.md) — panes, the focus interaction
  model, and the full key binding reference.
- [Using rinkaku with LLM reviewers](docs/llm-review.md) — a recipe for
  handing rinkaku's output to an LLM as a review "map."
- [Using the GitHub Action from another repository](docs/github-action.md) —
  workflow setup, inputs/outputs reference, and the trust-boundary
  considerations for the binary the action runs.
- [Architecture, known limitations, and roadmap](docs/architecture-and-limitations.md) —
  the ports & adapters layering, name-only dependency resolution's
  current limits, and the release process.
- [Architecture Decision Records](docs/adr) — the reasoning behind
  rinkaku's structural decisions.
- [Experiment 0001: map-assisted LLM review](docs/experiments/0001-map-assisted-llm-review/README.md) —
  evidence for the map-assisted review recipe above.

## Development

Requires a Rust toolchain (pinned in the
[toolchain file](rust-toolchain.toml); `rustup` will install it
automatically).

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + cargo clippy --all-targets --all-features -- -D warnings
make format  # cargo fmt --all
make help    # list available targets
```

CI runs the same `make` targets on every pull request (see the
[workflow definitions](.github/workflows)).

See [Architecture, known limitations, and roadmap](docs/architecture-and-limitations.md)
for the crate layout, the ports & adapters layering, and the release
process, and [contributor conventions](CLAUDE.md) for how this
repository expects changes to be made.

## License

MIT
