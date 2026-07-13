# rinkaku

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./assets/logo-dark.svg">
    <img alt="rinkaku logo" src="./assets/logo-light.svg" width="640">
  </picture>
</p>

> rinkaku (輪郭, "outline" in Japanese)

**See the shape of a PR before you read it.**

rinkaku takes any pull request (yours, a teammate's, an OSS one you
just found on GitHub) and shows you *what actually changed* — the
functions and types touched, what they call, what depends on them —
without making you scroll through the diff.

Two ways to look at it:

- 🖥️  **In your terminal** — an interactive TUI with a directory tree,
  a syntax-highlighted diff pane, and a *blast radius* view for
  *"if this changes, what does it reach?"*
  → [`docs/tui.md`](docs/tui.md)
- 🤖 **On the PR itself** — a GitHub Action that posts a sticky comment
  with a mermaid graph GitHub renders natively.
  → [`docs/action.md`](docs/action.md)

Both share one engine: tree-sitter extraction of changed symbols, 1-hop
dependency expansion, fan-in / contract-change / entry-point
summarizers. Rust, Go, Python, and TypeScript out of the box.

## Try it in 30 seconds

```sh
# 1. Install
brew install hiro-o918/tap/rinkaku
# or:
curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh | bash
# or install into your home directory (no sudo):
INSTALL_DIR="$HOME/.local/bin" bash -c "$(curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh)"

# 2. Point it at any GitHub PR — a local clone is optional
rinkaku --pr https://github.com/owner/repo/pull/123

# ...or the branch you're about to review
rinkaku --base main
```

Either command opens the TUI. Press `?` in the TUI for the keymap;
`q` to quit.

Prefer plain output? Pass `--format md`, `--format json`, or
`--format mermaid` — see [`docs/cli.md`](docs/cli.md).
Setting up CI so every PR gets a mermaid graph in a comment?
[`docs/action.md`](docs/action.md).
Feeding it to an LLM reviewer? Read
[`docs/llm-review.md`](docs/llm-review.md) first — it's an attention
aid, not a verifier, and we have the experiment rounds to prove it.

## How to read the TUI

The tree pane orders sibling directories topologically over the change
graph, not alphabetically: entry-point directories (nothing else
depends on them) sort first, heavily-depended-upon foundations sort
last (ADR 0016). Tests are excluded from that ranking entirely and
pinned into a trailing `Tests` section instead (ADR 0035). Files within
a directory are alphabetical; symbols keep source order. When a diff
has no cross-directory references, ranking has nothing to work with and
the order silently falls back to alphabetical — don't over-read
ordering in that case, and remember the underlying dependency edges
come from syntactic tree-sitter resolution, not a type checker, so a
reference can occasionally be missed (ADR 0003).

Badges on a row: `chg:N` changed symbols, `api:N` (yellow) contract/
signature changes, `fan-in:N` symbols that reference this one — i.e.
this row's *blast radius*, not a measure of its importance —
`lines:`/`warn:`/`split:` file-size discipline, and `(cycle)` for a
directory-level dependency cycle. A symbol's leading marker is `+`
added, `~` signature-changed, `x` removed, or blank for body-only.

A reading protocol that uses those signals instead of scrolling
top-to-bottom through the diff:

1. Read the tree top-down — callers appear before the callees they
   depend on.
2. Deep-read any row where `api:` and a high `fan-in:` co-occur: a
   changed contract with many referrers is the highest-risk zone in the
   PR.
3. Skim blank-marker (body-only) symbols — their signature didn't
   change, so they're locally contained.
4. Press `r`/`R` on a row you're suspicious of to pivot the right pane
   to its blast-radius tree and see everything downstream of it.
5. Treat `(cycle)` markers and file-size warnings as structural
   findings worth calling out separately, not as line-level bugs.

See [`docs/tui.md`](docs/tui.md) for the full layout and key-binding
reference.

## Install

| Method | Command |
| --- | --- |
| Homebrew | `brew install hiro-o918/tap/rinkaku` |
| Install script | `curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh \| bash` |
| From source | `cargo install rinkaku` |
| Manual | Grab a tarball from the [latest release](https://github.com/hiro-o918/rinkaku/releases/latest) and put `rinkaku` on your `PATH`. Targets: `{x86_64,aarch64}-{unknown-linux-gnu,apple-darwin}`. |

Update with `rinkaku self-update` (see
[`docs/cli.md#self-update`](docs/cli.md#self-update) for the
interactive / `--yes` / non-TTY behavior).

## Docs

- [`docs/tui.md`](docs/tui.md) — TUI layout, key bindings, navigation
- [`docs/action.md`](docs/action.md) — GitHub Action setup and
  trusted-base posture
- [`docs/cli.md`](docs/cli.md) — Every input mode, output format, and
  flag
- [`docs/llm-review.md`](docs/llm-review.md) — Recipe and honest
  caveats for LLM reviewers
- [`docs/adr/`](docs/adr) — Design decisions
- [`docs/experiments/`](docs/experiments) — Multi-round experiments
  (what actually worked, what didn't)

## Contributing

Requires a Rust toolchain (pinned in
[`rust-toolchain.toml`](rust-toolchain.toml); `rustup` installs it
automatically).

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + cargo clippy --all-targets --all-features -- -D warnings
make format  # cargo fmt --all
```

The workspace has three crates: `rinkaku-core` (pure library —
extraction and rendering; embeddable), `rinkaku-tui` (TUI view-models
and ratatui rendering), and `rinkaku` (the CLI). Core stays pure — no
IO, no clock, no env — with tree-sitter and future LSP boundaries
isolated behind consumer-side traits. See [`CLAUDE.md`](CLAUDE.md) and
[`docs/adr/`](docs/adr) for the reasoning.

## License

MIT
