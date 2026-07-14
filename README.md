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

```
v ! auth chg:4 api:2 fan-in:3   <- contract change + high fan-in
    ! token.rs api:1 fan-in:2   <- same, rolled up to the file
      ~ fn ! verify_token       <- changed + high fan-in itself
      + fn rotate_secret        <- added
        fn log_attempt          <- body-only, dimmed: skim
    > 6 tests                   <- folded tests, dimmed
v tui                           <- no risk marker here
    render.rs lines:812         <- file-size watch badge
  legacy  (cycle)               <- dependency cycle
```

Top-down = callers before callees: entry points sort first, foundations
last. `fan-in:` counts other *production* symbols referencing this one,
never tests. Press `r`/`R` on any row to pivot the right pane to its
blast radius; `?` opens the full keymap.

See [`docs/tui.md`](docs/tui.md) for the full layout, key bindings, and
caveats on ordering and dependency resolution.

### Leaving review notes

Press `n` over a symbol row to attach a note; `N` opens the notes list
(`j`/`k` to move, `Enter` to export, `d` to delete). Export goes to
whichever sinks apply to how you launched the TUI:

- **GitHub PR review** — only when you started with `rinkaku --pr
  <url or number>`. Picking this opens a verdict menu (Approve /
  Request changes / Comment) and posts every note as one batched PR
  review.
- **Clipboard, for an AI agent** — always available. Copies a Markdown
  packet (one section per note, with the note's own symbol signature)
  via an OSC 52 terminal escape sequence, so it works over SSH too.

Press `w` from anywhere to open the PR's page in your web browser
(only when you started with `rinkaku --pr <url or number>`).

## Install

| Method | Command |
| --- | --- |
| Homebrew | `brew install hiro-o918/tap/rinkaku` |
| Install script | `curl -fsSL https://raw.githubusercontent.com/hiro-o918/rinkaku/main/install.sh \| bash` |
| From source | `cargo install rinkaku` |
| Manual | Grab a tarball from the [latest release](https://github.com/hiro-o918/rinkaku/releases/latest) and put `rinkaku` on your `PATH`. Targets: `{x86_64,aarch64}-{unknown-linux-musl,unknown-linux-gnu,apple-darwin}` (Linux tooling picks the statically linked musl build). |

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
