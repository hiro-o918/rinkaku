# rinkaku

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
dependency expansion, hotspot / contract-change / entry-point
summarizers. Rust, Go, Python, and TypeScript out of the box.

## Try it in 30 seconds

```sh
# 1. Install
brew install hiro-o918/tap/rinkaku
# or:
curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh | bash

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
