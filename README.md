# rinkaku

> rinkaku (輪郭, "outline" in Japanese)

**A map of what a PR changes** — browse it interactively in your terminal,
or read it inline in the PR itself as a mermaid graph.

For any pull request rinkaku picks out the *changed symbols* (and their
1-hop dependencies), turns them into a compact call/dependency graph, and
renders that graph two ways:

1. **[Interactive TUI](#interactive-tui-recommended)** — a terminal-native
   viewer with a directory tree, a diff pane, and a "blast radius" pane
   for asking "if this changes, what does it reach?" Built for humans
   reading a PR before diving into files.
2. **[GitHub Action](#github-action)** — a composite action that posts (or
   updates) a sticky PR comment with a mermaid graph rendered natively by
   GitHub, plus the full signature outline collapsed underneath.

Both are backed by the same core: tree-sitter-based extraction of changed
symbols, 1-hop dependency expansion, and structural summarizers
(hotspots, contract markers, entry-point trees, blast radius). Rust, Go,
Python, and TypeScript are supported out of the box.

## Status

Early development. Diff parsing, tree-sitter extraction, the CLI
(stdin/`--base`/`--pr` input, Markdown/JSON/Mermaid output), 1-hop
dependency expansion (`--deps`, the tags-based `Resolver`), the
interactive TUI (`--tui`), and the GitHub Action are all implemented.

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

When stdin is not a terminal (e.g. run from a script or CI) and `--yes`
is not given, `self-update` refuses to run rather than silently
proceeding.

## Interactive TUI (recommended)

The TUI is the intended path for humans reading a PR
([ADR 0015](docs/adr/0015-tui-for-humans-markdown-for-machines.md));
it's built with [ratatui](https://ratatui.rs)
([ADR 0016](docs/adr/0016-tui-crate-and-stack.md)) and follows a
neovim-style focus model
([ADR 0020](docs/adr/0020-tui-interaction-model-v2.md)).

```sh
# Whole-repo outline — bare invocation opens the TUI on an interactive
# terminal (ADR 0017), useful for onboarding or architecture review
rinkaku

# A PR by number (inside a local clone of the target repository)
rinkaku --pr 123 --tui

# A PR from any directory (URL form; auto-picks an existing ghq clone or
# auto-clones a blobless copy into a cache — see ADR 0005/0006)
rinkaku --pr https://github.com/octocat/hello-world/pull/123 --tui

# A local git diff against a base branch
rinkaku --base main --tui

# Diff piped in from anywhere (keyboard input is read from the
# controlling terminal independently of stdin — ADR 0016)
gh pr diff 123 | rinkaku --tui
```

`--tui` replaces `--format`, so the two conflict rather than combining.

### Layout

The screen is split into a **tree pane** on the left and one of three
**right-hand panes**:

- **Tree pane** — a directory tree of *changed files* (not the call
  graph), so nesting mirrors your repository layout. Rows carry badges:
  `~N` changed symbols, `!N` contract changes (added/removed/signature-
  changed), `^N` fan-in (hotspot aggregate). Directories that
  participate in a dependency cycle are marked `(cycle)`. Sibling
  directories default to topological order — entry points (least
  depended-on) first, foundations last — with `o` to toggle plain
  alphabetical order. Symbol rows show a kind abbreviation (`fn`,
  `struct`, ...) and a classification marker: `+` added, `~`
  signature-changed, `x` removed (dimmed, crossed out). Files rinkaku
  couldn't analyze (unsupported language, binary, deleted) still
  appear, dimmed and marked `(skipped: <reason>)`; a file whose changes
  were entirely test code shows up as `[test] (N symbols)` (ADR 0009).
- **Diff pane (default right pane)** — the raw unified-diff hunks
  touching the selected row. Symbol rows clip to that symbol's line
  range; file rows group hunks under per-symbol section headers, with
  import-only hunks collected under a trailing `(module level)`
  section. When a symbol's signature itself changed, a 2-line old/new
  header (`- <old>` / `+ <new>`) precedes its hunks. Hunks in the four
  built-in languages are syntax-highlighted
  ([ADR 0018](docs/adr/0018-syntax-highlight-tui-diff-pane.md));
  added/removed lines keep their green/red diff signal as a background
  tint so it doesn't compete with token colors. Any other file falls
  back to plain green/red text.
- **Detail pane** (`d`) — what the cursor is on. A symbol row shows its
  classification, signature (an old/new diff when the contract
  changed), who depends on it ("used by"), and its callees. A file row
  shows every symbol changed in that file. A directory row shows its
  badge breakdown, top fan-in symbols, and — when it participates in a
  dependency cycle — exactly which other directories it cycles with and
  the concrete symbol-to-symbol edges forming that cycle.
- **Blast radius pane** (`r`) — an entry-tree view rooted at the
  directory or file row under the cursor: *"if this changes, what does
  it reach?"* The interactive equivalent of the CLI's `--entry <path>`
  ([ADR 0019](docs/adr/0019-entry-path-pivot-view.md), named per
  [ADR 0023](docs/adr/0023-tui-blast-radius-naming.md)). The tree
  follows the cursor: move to a different row and it re-renders rooted
  at the new path. Nodes reached only past the selected path are
  dimmed, repeated nodes are marked `(see above)` instead of expanded
  again, and cycles are marked `(cycle)`.

### Key bindings

Press `?` in the TUI for the always-up-to-date version, grouped by
focus.

**Tree focus (default):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Move the cursor |
| `enter` | Expand/collapse a directory row, or open a file/symbol row (moves focus right) |
| `space` | Expand or collapse a directory/file row (never moves focus) |
| `e` / `E` | Expand every row |
| `c` / `C` | Collapse every row |

**Right focus (Detail/Diff/Blast-radius pane):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Scroll the pane by one line |
| `h` / `esc` | Return focus to the tree |
| `]` | Jump to the next hunk (Diff pane only) |
| `[` | Jump to the previous hunk (Diff pane only) |

**Global (any focus):**

| Key(s) | Action |
| --- | --- |
| `o` / `O` | Toggle topological / alphabetical ordering |
| `d` / `D` | Toggle the right-hand pane between diff and detail |
| `r` / `R` | Toggle the right-hand pane to the blast radius of the selected directory/file |
| `s` / `S` | Open the source view for the symbol under the cursor |
| `gd` | Jump to a callee of the symbol under the cursor ([ADR 0022](docs/adr/0022-jump-navigation-and-jumplist.md)) |
| `gr` | Jump to a caller of the symbol under the cursor |
| `ctrl-o` | Jump back to the previous jumplist location |
| `ctrl-i` | Jump forward to the next jumplist location |
| `?` | Toggle the help overlay (keymap + glossary) |
| `esc` / `q` | Return to the entry view (from the source view) |
| `q` / `ctrl-c` | Quit (from the entry view) |

Glyphs are plain ASCII (`~`/`!`/`^`/`+`/`x`, `v`/`>` for expand state)
rather than Unicode/emoji, for compatibility with plainer terminal
configurations.

### Jump navigation (`gd` / `gr`)

While reading a diff or its detail, `gd` jumps toward a callee of the
symbol under the cursor and `gr` jumps toward a caller — a two-key
sequence (press `g` then `d`/`r`), mirroring neovim's own "go to
definition"/"go to references" idiom
([ADR 0022](docs/adr/0022-jump-navigation-and-jumplist.md)). Zero
candidates shows a status-line note, one candidate jumps immediately,
and more than one opens a popup (`j`/`k` to choose, `enter` to jump,
`esc` to cancel). A jump moves the tree cursor to the target symbol —
expanding any collapsed ancestor directories along the way — and shows
its Diff, without moving focus off the right pane.

Every jump is recorded in a jumplist: `ctrl-o` returns to where you
jumped from, and `ctrl-i` moves forward again after a `ctrl-o` — the
same back/forward history neovim's own jumplist keeps. Jumping to a new
location from the middle of that history discards whatever forward
entries existed.

### Source view

`s` on a symbol row opens the file, scrolled to and highlighting the
symbol's line range; `esc`/`q` returns to the entry view. Reads the
working tree directly (not the historical commit a `--base`/`--pr`
diff was computed against), so it always shows the file's current
content — note that the highlighted line range itself is from analysis
time, so it can drift (or, if the file has since shrunk, get clamped to
the end of the file) if you edit the file after opening the TUI.

## GitHub Action

The composite [`action.yaml`](action.yaml) runs rinkaku against a pull
request's diff and posts (or updates) a sticky PR comment: a
[`--format mermaid`](#format-mermaid-graph-for-pr-comments)
call/dependency graph up front — rendered natively by GitHub in the
comment — with the full Markdown outline collapsed underneath for
anyone who wants signature-level detail.

```yaml
name: rinkaku PR report

on:
  pull_request:
    branches: [main]

permissions:
  pull-requests: write
  contents: read

jobs:
  report:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0

      - name: Fetch base branch
        run: git fetch origin ${{ github.event.pull_request.base.ref }}

      - uses: hiro-o918/rinkaku@main
        with:
          github-token: ${{ github.token }}
```

Inputs: `version` (a release tag to download, default `latest`),
`binary` (path to an already-built rinkaku binary, skipping the
download entirely), `repo-path` (the checkout rinkaku should analyze;
defaults to the current directory), `base` (defaults to the PR's base
ref), `github-token` (defaults to `github.token`), and `comment` (set
`false` to skip posting and just get the `mermaid-path`/`markdown-path`
outputs).

### Trusted-base posture

The snippet above is the simple case (a pinned release binary,
`permissions: pull-requests: write` scoped to only what posting a
comment needs). If you build rinkaku yourself instead of using a
release binary, **build it from the PR's base ref, not the PR head** —
same rule this repository's own [dogfooding
workflow](.github/workflows/rinkaku-report.yaml) follows, and for the
same reason the [LLM-review recipe](#advanced-using-rinkaku-with-llm-reviewers)
below always builds its map from a trusted checkout: a PR is exactly
the input an attacker controls, and this job runs with a write token
before anyone has reviewed it. That workflow checks out the PR's base
ref at the job's default location (so `uses: ./` resolves *its own
action code* — not just the binary — from the trusted checkout too)
and checks out the PR head into a subdirectory purely as data, passed
to this action via `repo-path`.

**Fork PRs** get a read-only token from the `pull_request` trigger
regardless of `permissions:` — this action detects that and falls back
to writing the report into the job's step summary instead of posting a
comment, so a fork PR's run still succeeds (exit 0) rather than
failing on a 403.

## CLI reference

The TUI and Action are the recommended entry points, but the same
analysis is available as plain CLI output — useful for scripting, CI
gates, custom tooling, or feeding downstream processors.

### Input modes

- **stdin**: `gh pr diff 123 | rinkaku` — file contents are read off
  the working tree, which assumes the piped diff is consistent with
  the current working tree (e.g. it was just produced by `git diff`
  or already applied). A stale or unrelated diff piped via stdin can
  produce misaligned line numbers.
- **`--base <ref>`**: `rinkaku --base main` — runs `git diff`
  internally and reads file contents via `git show <head>:<path>`, so
  extraction always matches the diffed commit regardless of the
  working tree's state.
- **`--pr <number-or-url>`**: `rinkaku --pr 123` or
  `rinkaku --pr https://github.com/owner/repo/pull/123` — resolves
  the PR's base and head via `gh pr view`, fetches both, and reuses
  the same `git show`-backed read strategy as `--base`
  ([ADR 0004](docs/adr/0004-pr-input-mode-via-gh-in-local-clone.md)).
  A bare PR number requires running inside a local clone of the
  target repository; a URL also works from any directory, preferring
  an existing [ghq](https://github.com/x-motemen/ghq)-managed clone
  when one matches and otherwise auto-cloning a blobless copy into a
  cache ([ADR 0005](docs/adr/0005-auto-clone-into-cache-for-pr-urls.md),
  [ADR 0006](docs/adr/0006-prefer-ghq-managed-clones-over-cache.md)).
  `gh` must be installed and authenticated. Private repos also need
  `gh auth setup-git` so later `git fetch`s can authenticate.
- **Whole-repo (no diff)**: bare `rinkaku` run inside a repository,
  with stdout piped anywhere — no diff involved, produces an outline
  of every symbol and its dependency structure
  ([ADR 0017](docs/adr/0017-whole-repo-outline-as-default-input-mode.md)).
  On an interactive terminal, bare `rinkaku` opens the TUI instead
  (same whole-repo outline); pass `--format md` to force Markdown
  even on a TTY.

### Output formats (`--format`)

```sh
# Markdown (default when stdout is not a TTY) — designed to be fed to
# an LLM or read by a human reviewer
rinkaku --base main --format md

# JSON — structured data for another tool
rinkaku --base main --format json

# Mermaid — a call/dependency flowchart for pasting into a PR
# comment/description (ADR 0021), where GitHub renders it natively
rinkaku --base main --format mermaid
```

`--tui` is not a `--format` value; it replaces the output stage
entirely and conflicts with `--format`.

### Markdown output example

Running `rinkaku` on
[a real rinkaku commit](https://github.com/hiro-o918/rinkaku/commit/aa7ca34)
(a 35-line diff adding stderr progress logging to `main.rs`) produces:

```sh
$ git show aa7ca34 --format="" | rinkaku --deps 0
```

````markdown
## Change graph

4 changed symbols in 1 file

- fn main (rinkaku/src/main.rs)
  - fn build_resolver (rinkaku/src/main.rs)
  - fn resolve_pr_workdir (rinkaku/src/main.rs)
  - fn run_base_pipeline (rinkaku/src/main.rs)
    - fn build_resolver (rinkaku/src/main.rs) (see above)

## Definitions

### fn main (rinkaku/src/main.rs)

```
fn main() -> anyhow::Result<()>
```

### fn build_resolver (rinkaku/src/main.rs)

```
fn build_resolver( cli: &Cli, diff_text: &str, diff_read_file: impl Fn(&str) -> std::io::Result<String>, head: Option<&str>, cwd: Option<&std::path::Path>, ) -> anyhow::Result<Option<TagsResolver>>
```

### fn resolve_pr_workdir (rinkaku/src/main.rs)

```
fn resolve_pr_workdir(parsed: &PrArg) -> anyhow::Result<Option<std::path::PathBuf>>
```

### fn run_base_pipeline (rinkaku/src/main.rs)

```
fn run_base_pipeline( cli: &Cli, base: &str, head: &str, cwd: Option<&std::path::Path>, ) -> anyhow::Result<rinkaku_core::render::Report>
```

````

The line under the heading summarizes the shape of the change — how
many symbols changed, across how many files, and (for multi-file
diffs) which file concentrates most of them, e.g.
`16 changed symbols in 3 files — most in store/items.go (11)` — so a
reviewer sees the epicenter before reading a single tree line.

"Change graph" reads top-down in call-hierarchy order
([ADR 0008](docs/adr/0008-entry-point-tree-rendering-for-changed-symbols.md)):
`main` is the only entry point, and every function it reaches is
nested beneath it. `build_resolver` is reachable from both `main` and
`run_base_pipeline`; it is rendered in full once, under `main` (the
first place it's reached), and referenced by name only (`(see above)`)
the second time — so a reviewer never sees the same signature twice.
When two changed symbols depend on each other (a mutual-recursion
cycle), the edge that closes the loop is marked
`⚠️ ... — dependency cycle, see above` instead of being walked into
again.

Two more condensations
([ADR 0012](docs/adr/0012-condense-change-graph-rendering.md)) keep
request/response-style diffs readable. Changed **data-carrier types**
(structs/enums/type aliases with no outgoing edges of their own) don't
get tree lines; they're folded into the line of each symbol that
references them as a `— uses:` annotation, with their full signatures
still listed under "Definitions". And an **interface/trait declaration
is linked to its changed methods** by method-spec name, so the methods
nest under the interface instead of duplicating it at top level. A Go
diff adding an `ItemStore` interface, two receiver-method
implementations, and four request/response structs — 8 changed symbols
— renders as just three tree lines:

```markdown
## Change graph

8 changed symbols in 1 file

- interface ItemStore (store.go) — uses: ListItemsRequest, ListItemsResponse, SaveItemRequest, SaveItemResponse
  - fn ListItems (store.go) — uses: ListItemsRequest, ListItemsResponse, itemStore
  - fn SaveItem (store.go) — uses: SaveItemRequest, SaveItemResponse, itemStore
```

Unchanged 1-hop dependencies (ADR 0003) — functions/types the diff
touches but did not itself change — show up as a `Depends on:` list
under each definition; they're omitted from the example above
(`--deps 0`) to keep it focused on the tree shape.

### JSON output

`--format json` renders the same data as structured JSON
(`{"files": [...], "skipped": [...], "graph": {"nodes", "edges", "roots"}}`),
for piping into `jq` or another tool. The `graph` field is the same
call-graph data "Change graph" renders as a tree, so JSON consumers
don't need to recompute it from `referenced_names`:

```sh
$ git show aa7ca34 --format="" | rinkaku --format json --deps 0 | jq '.graph'
```

```json
{
  "nodes": [
    { "id": "rinkaku/src/main.rs::main", "path": "rinkaku/src/main.rs", "name": "main" },
    { "id": "rinkaku/src/main.rs::resolve_pr_workdir", "path": "rinkaku/src/main.rs", "name": "resolve_pr_workdir" },
    { "id": "rinkaku/src/main.rs::run_base_pipeline", "path": "rinkaku/src/main.rs", "name": "run_base_pipeline" },
    { "id": "rinkaku/src/main.rs::build_resolver", "path": "rinkaku/src/main.rs", "name": "build_resolver" }
  ],
  "edges": [
    { "from": "rinkaku/src/main.rs::main", "to": "rinkaku/src/main.rs::build_resolver", "is_cycle": false },
    { "from": "rinkaku/src/main.rs::main", "to": "rinkaku/src/main.rs::resolve_pr_workdir", "is_cycle": false },
    { "from": "rinkaku/src/main.rs::main", "to": "rinkaku/src/main.rs::run_base_pipeline", "is_cycle": false },
    { "from": "rinkaku/src/main.rs::run_base_pipeline", "to": "rinkaku/src/main.rs::build_resolver", "is_cycle": false }
  ],
  "roots": ["rinkaku/src/main.rs::main"]
}
```

Each symbol in `files[].symbols` also carries an `id` field matching
its `graph` node's `id`, so a consumer can join a symbol's full
signature back to its position in the graph without recomputing the
`{path}::{name}` id scheme itself.

The top-level JSON report also carries `"tests": [{"path", "symbol_count"}]`
(ADR 0009's per-file test-symbol counts, empty unless the diff touched
any) and `skipped[].reason` can be `"generated"` (ADR 0010
attribute-based detection, or ADR 0011 content-marker detection — both
report the same reason value) alongside the existing
`"unsupported_language"`/`"binary"`/`"deleted"` values.

### `--format mermaid` (graph for PR comments)

`--format mermaid` emits the same call/dependency graph as a mermaid
flowchart
([ADR 0021](docs/adr/0021-mermaid-output-format.md)), designed for
pasting into a GitHub PR comment or description where mermaid renders
natively. This is the format the [GitHub Action](#github-action) uses
for the top of its sticky comment.

### `--deps`

`--deps 1` (the default) resolves each changed symbol's 1-hop
dependencies by indexing every file `git ls-files` tracks in the
repository — this makes the output more useful (you see what a changed
function calls) but costs an up-front repo-wide scan. On a large
diff/repo, `--deps 1` can take several seconds versus ~50ms for
`--deps 0`. Prefer `--deps 0` for quick iteration or CI checks where
the dependency context isn't needed. See
[Known limitations](#known-limitations) for the resolver's precision
caveats.

### `--exclude-tests`

By default ([ADR 0025](docs/adr/0025-default-to-including-tests.md)),
test symbols appear in "Change graph"/"Definitions" alongside
production symbols, and the `## Tests` summary section is omitted —
this shape is designed for LLM consumers of the Markdown/JSON output,
for which "which contracts have which tests changed alongside them"
is useful signal rather than noise.

Pass `--exclude-tests` to opt into the previous behavior: test symbols
are detected per language (Go's `*_test.go`, Python's
`test_*.py`/`*_test.py` and `tests/` directories, TypeScript's
`*.test.ts(x)`/`*.spec.ts(x)` and `__tests__/`, and Rust's `tests/`
directory plus `#[cfg(test)]` modules and
`#[test]`/`#[rstest]`/`#[tokio::test]`-attributed functions) and
summarized instead as a per-file count under a `## Tests` section:

```markdown
## Tests

- rinkaku-core/src/pipeline.rs: 3 changed test symbols
```

Under `--exclude-tests`, `TagsResolver`'s repo-wide dependency index
applies the same exclusion, so a changed production symbol's "Depends
on:" cannot resolve to a same-named test helper or fixture elsewhere
in the repo (ADR 0009).

### `--include-generated`

By default, rinkaku skips generated files two ways, both controlled by
this one flag:

- **`.gitattributes`** ([ADR 0010](docs/adr/0010-skip-files-marked-no-diff-or-generated-in-gitattributes.md)):
  resolves each changed file's `diff`/`linguist-generated` attributes
  via `git check-attr` and skips files marked `-diff` or
  `linguist-generated`. Only applies when a local git repository is
  available.
- **Content markers** ([ADR 0011](docs/adr/0011-detect-generated-files-by-content-markers.md)):
  skips a file whose first 5 lines contain a linguist-compatible
  marker: a `@generated` comment, or a line with both `Code generated`
  and `DO NOT EDIT` (Go tooling/protobuf's
  `// Code generated by <tool>. DO NOT EDIT.` convention, matched
  regardless of the comment syntax a language uses). Doesn't require a
  git repository.

Either way, skipped files (lockfiles, generated code) are dropped from
Markdown output silently: they do **not** appear under "Skipped files"
at all. A diff that touches nothing but generated files renders as
empty Markdown output.

They do still appear in `--format json`'s `skipped` array, with reason
`"generated"`, for consumers that want the full picture:

```sh
$ rinkaku --base main --format json | jq '.skipped'
```

```json
[
  { "path": "Cargo.lock", "reason": "generated" },
  { "path": "models/user.go", "reason": "generated" }
]
```

Pass `--include-generated` to restore the previous behavior (generated
files — by either detection method — are analyzed like any other file,
in both output formats).

### `--entry <path>`

By default, "Change graph"/"Repository graph" is rooted at
auto-detected entry points (ADR 0008): symbols nothing else in the
graph depends on. `--entry <path>` re-roots the tree at a chosen path
instead ([ADR 0019](docs/adr/0019-entry-path-pivot-view.md)) — entry
points become the symbols under `path` that nothing else *under that
same path* depends on, and dependency trees still expand outward
through the whole graph as usual. This is a change of vantage point,
not a filter: symbols outside `path` are neither hidden nor excluded
from analysis, only no longer eligible to be roots themselves.

```sh
rinkaku --base main --entry src/api
```

Combines with `--tui`: the TUI opens with the cursor already on the
tree row matching `path` and the right-hand pane already showing its
Blast radius (the `R` binding). If no tree row's path matches exactly,
the TUI opens normally with a status-line note instead.

Prints `note: no symbols under <path>` to stderr and renders an empty
tree when no symbol's path falls under `path`. Fan-in counts remain
whole-analysis under `--entry` / the TUI blast-radius pane —
re-rooting the tree changes the vantage point, not the direction
fan-in itself measures (see
[ADR 0019](docs/adr/0019-entry-path-pivot-view.md)'s Consequences).

## Advanced: using rinkaku with LLM reviewers

rinkaku's output can also be handed to an LLM as a "map" before it
reads a diff. Ten rounds of a paired-arm experiment (see
[`docs/experiments/0001-map-assisted-llm-review/`](docs/experiments/0001-map-assisted-llm-review/README.md))
found that the map is a **complement, not a substitute** for a plain
diff review, and that its measurable value shows up as
**attention allocation** (routing toward integration seams,
self-consistency defects, and coverage boundaries) rather than as
token savings. Neither pass produced a superset of the other's
findings, and dynamic verification (building and executing the
changed code, especially against hostile/edge-case inputs) remained
the strongest single predictor of finding real behavioral defects.

If you still want to run it, the recipe below reflects those
constraints:

1. Generate the map from a **trusted checkout** — a clean `main`
   build, never the branch under review, so a malicious or buggy diff
   can't tamper with the tool inspecting it:

   ```sh
   rinkaku --pr 123 --format md > map.md
   # or: rinkaku --base main --format md > map.md
   ```

2. Paste `map.md` at the top of the reviewer's prompt, followed by
   the actual diff, with instructions along these lines:

   ```
   Here is a structural map of this change (hotspots, contract markers,
   entry-point trees). Use it to decide where to read deeply first, but
   it is an attention-allocation aid, not a verifier: read the full
   implementation of anything it flags, and don't assume unflagged code
   is safe to skip. Then review the diff below.
   ```

3. Run an **independent pass without the map** alongside the
   map-assisted one; the two consistently surface different findings.

4. Add a **dynamic verification** step: build and actually execute
   the changed code, including failure-mode invocations (non-TTY
   stdin, empty input, missing files). Behavioral bugs don't show up
   on the signature surface the map draws from.

## Development

Requires a Rust toolchain (pinned in
[`rust-toolchain.toml`](rust-toolchain.toml); `rustup` will install it
automatically).

The workspace has three crates: `rinkaku-core` (the pure
diff-condensation library, published standalone so it can be embedded
in other tools), `rinkaku-tui` (the interactive terminal UI's
view-models and `ratatui` rendering, depending on `rinkaku-core`; see
[ADR 0016](docs/adr/0016-tui-crate-and-stack.md)), and `rinkaku` (the
thin CLI binary, depending on both).

```sh
make test    # cargo test --all-features
make lint    # cargo fmt --check + cargo clippy --all-targets --all-features -- -D warnings
make format  # cargo fmt --all
make help    # list available targets
```

CI runs the same `make` targets on every pull request (see
[`.github/workflows/`](.github/workflows)).

### Architecture

Lightweight ports & adapters: core extraction logic in `rinkaku-core`
is pure (no IO, no clock, no env), with tree-sitter parsing and future
LSP/process boundaries isolated behind traits (`LanguageSupport`,
`Resolver`) defined on the consumer side. See
[`CLAUDE.md`](CLAUDE.md) and [`docs/adr/`](docs/adr) for details.

### Known limitations

**Mitigated in [#9](https://github.com/hiro-o918/rinkaku/pull/9):** the
original QA pass found name-only matching noise and slow `--deps 1`
indexing severe enough to block merging. Both are improved, though not
eliminated — v1's resolver is still name-only (see "still open" below).

- **Same-name matches are ranked and capped, not resolved.** When
  several definitions share a referenced name, they are ranked by path
  proximity to the referencing file (same file > same directory >
  shared path prefix depth > other) and only the top 3
  ([`MAX_MATCHES_PER_NAME`]) are shown; the rest are reported as a
  count (`(+N more definitions matched by name)` in Markdown,
  `omitted_matches` in JSON). This bounds "Depends on" noise but does
  not guarantee the top 3 include the actually-referenced definition,
  since ranking is a proximity heuristic, not type-aware resolution.
- **`_` and single-character identifiers are never resolved.** They
  are filtered out of referenced names entirely, since under name-only
  resolution they match too many unrelated definitions to be useful
  (Python's `_` placeholder convention was the main offender).
- **The `--deps 1` indexing prefilter has limited effect when a diff
  references common standard-library-style names.**
  `TagsResolver::new` skips parsing files whose content cannot contain
  any referenced name at all (measured ~88% fewer files parsed, ~8x
  faster indexing on a same-language-only reference set — see PR #9
  for the full numbers). But a name like `Vec`, `Option`, `String`,
  `Some`, or `Ok` appears in nearly every Rust file in a real
  codebase, so a diff whose referenced names include several of these
  sees a smaller reduction. The prefilter is a substring match over
  raw file content, not scoped to actual definitions, so it cannot
  distinguish "defines `Vec`" from "mentions `Vec`" without also
  risking false negatives. The dominant cost in `--base` mode remains
  the per-file `git show` subprocess invocation for reading tracked
  files (unaddressed).

**Still open — no type resolution (by design, ADR 0003):** dependency
resolution matches referenced names against definitions by name alone,
with no type information — it cannot disambiguate overloads, shadowed
names, or same-named symbols in unrelated modules. The ranking and cap
above reduce the resulting noise but do not fix the underlying
imprecision. A future `Resolver` implementation backed by an LSP
server (pyright, gopls, rust-analyzer, ...) is planned as a
higher-precision, opt-in alternative for v2+; see the Roadmap below.

[`MAX_MATCHES_PER_NAME`]: rinkaku-core/src/deps.rs

### Roadmap / not yet done

- LSP-backed `Resolver` implementations (pyright, gopls,
  rust-analyzer, ...) as a higher-precision, opt-in alternative to
  the v1 tags-based `Resolver`.
- A TUI screenshot/GIF in this README — deferred to a follow-up (the
  recording tooling needs a controlling TTY that this repository's
  development environment doesn't always provide).

## Release

`rinkaku` and `rinkaku-core` are versioned independently by
[release-please](https://github.com/googleapis/release-please) (no
`linked-versions` grouping): each crate only bumps when a commit
touches its own path, so it's normal for them to be on different
versions (e.g. `rinkaku` 0.2.0 depending on `rinkaku-core` 0.1.0).
Only `rinkaku`'s release tag (`v{version}`, no component prefix)
triggers `build-and-publish.yaml`; `rinkaku-core`'s tag is prefixed
(`rinkaku-core-v{version}`) so a `rinkaku-core`-only release doesn't
spin up the binary build/publish pipeline.

`separate-pull-requests: true` is set for a reason that isn't obvious
from the config alone: with more than one non-root `packages` entry
(no `.` path), release-please's PR-merging step can't find a "root"
release candidate to base the combined PR's title on, and falls back
to a title that omits the version entirely (`chore: release main`).
That title doesn't match what the *next* run expects when looking up
the already-merged PR to tag, so tagging silently finds nothing to do
and `release-main.yaml` aborts with "untagged, merged release PRs
outstanding" -- this bit us for both the v0.2.0 and v0.3.0 releases,
each requiring a manual `gh release create` + relabeling the PR
`autorelease: tagged` to recover. `separate-pull-requests: true`
sidesteps this entirely: each package gets its own PR (and its own
title, correctly including that package's version), so there's no
combined-PR title to compute in the first place.

## License

MIT
