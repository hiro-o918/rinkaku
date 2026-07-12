# rinkaku

> rinkaku (輪郭, "outline" in Japanese)

A CLI that condenses large PR diffs — especially LLM-generated ones — into
just the **signatures of changed symbols and their dependencies**, so
reviewers and LLMs can grasp the API surface of a change without reading
every implementation line.

## What it is

- **Input**: a unified diff via stdin (`gh pr diff 123 | rinkaku`),
  `rinkaku --base main` to run `git diff` internally, or
  `rinkaku --pr 123` to review a GitHub PR directly. In `--base` mode,
  file contents are read via `git show <head>:<path>` so extraction always
  matches the diffed commit, regardless of the working tree's state.
  `--pr` mode resolves the PR's base and head via `gh pr view`, fetches
  both, and reuses the same `git show`-backed read strategy as `--base`
  (see [ADR 0004](docs/adr/0004-pr-input-mode-via-gh-in-local-clone.md)).
  A bare PR number requires running inside a local clone of the target
  repository; a URL also works from any directory, preferring an existing
  [ghq](https://github.com/x-motemen/ghq)-managed clone when one matches
  and otherwise auto-cloning a blobless copy into a cache (see
  [ADR 0005](docs/adr/0005-auto-clone-into-cache-for-pr-urls.md) and
  [ADR 0006](docs/adr/0006-prefer-ghq-managed-clones-over-cache.md)). `gh`
  must be installed and authenticated either way. In
  stdin mode, file contents are read off the working tree, which assumes
  **the piped diff is consistent with the current working tree** (e.g. it
  was just produced by `git diff` or already applied); a stale or
  unrelated diff piped via stdin can produce misaligned line numbers.
- **Core**: tree-sitter parses the changed files, finds the definitions
  that contain changed lines, and slices out their signatures.
- **Dependency expansion**: each changed symbol is expanded one hop out to
  the definitions it references, via tree-sitter tags queries (v1).
  LSP-based resolvers (pyright, gopls, etc.) are a pluggable extension
  point (`Resolver` trait) planned for a later release. Same-name matches
  are ranked by path proximity to the referencing file and capped at 3 to
  keep "Depends on" readable; see Known limitations below.
- **Languages (v1, built-in)**: Rust, Go, Python, TypeScript. Each is a
  `LanguageSupport` trait implementation (grammar crate + tags query +
  signature-slicing rule), so language support is additive.
- **Output**: Markdown or JSON, designed to be fed to an LLM or read by a
  human reviewer.

See [`docs/adr/`](docs/adr) for the reasoning behind these choices.

## Status

Early development. Diff parsing, tree-sitter extraction, the CLI
(stdin/`--base`/`--pr` input, Markdown/JSON output), and 1-hop dependency
expansion (`--deps`, the tags-based `Resolver`) are implemented.

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

## Usage

```sh
# Bare invocation, run interactively inside a repository: no diff
# involved, opens the TUI with a whole-repo outline — every symbol and
# its dependency structure, for onboarding or architecture review
# (see ADR 0017)
rinkaku

# Same whole-repo outline, printed as Markdown instead of opening the TUI
# (this also happens automatically whenever stdout isn't a terminal, e.g.
# `rinkaku > outline.md`)
rinkaku --format md

# From a GitHub PR (stdin, no local clone required)
gh pr diff 123 | rinkaku

# From a GitHub PR by number: run inside a local clone of the target
# repository (fetches the PR via `gh`/`git`; requires `gh` installed and
# authenticated)
rinkaku --pr 123

# From a GitHub PR URL: works from any directory. If the cwd isn't already
# a clone of that repository, rinkaku prefers an existing ghq-managed
# clone (see ADR 0006) when ghq is installed, else auto-clones a blobless
# copy into $RINKAKU_CACHE_DIR / $XDG_CACHE_HOME/rinkaku / ~/.cache/rinkaku
# and runs there instead (see ADR 0005). Private repos need
# `gh auth setup-git` so later `git fetch`s can authenticate too.
rinkaku --pr https://github.com/octocat/hello-world/pull/123

# From a local git diff against a base branch
rinkaku --base main

# JSON output for feeding into another tool or LLM
rinkaku --base main --format json

# A human-oriented call/dependency graph as a mermaid flowchart (ADR
# 0020) — opt-in, meant for pasting into a GitHub PR comment/description
# where mermaid renders natively, not for piping into an LLM
rinkaku --base main --format mermaid

# Skip dependency resolution (faster, no repo-wide index — see below)
rinkaku --base main --deps 0
```

### What it looks like

All examples below are captured from the `cargo build --release` binary
post-[ADR 0008](docs/adr/0008-entry-point-tree-rendering-for-changed-symbols.md)
(entry-point tree rendering) and
[ADR 0012](docs/adr/0012-condense-change-graph-rendering.md)
(condensed rendering), not fabricated. The example diff below
touches no test symbols and no generated files, so it renders the same
with or without the defaults introduced in
[ADR 0009](docs/adr/0009-exclude-test-symbols-from-output-by-default.md),
[ADR 0010](docs/adr/0010-skip-files-marked-no-diff-or-generated-in-gitattributes.md),
and [ADR 0011](docs/adr/0011-detect-generated-files-by-content-markers.md)
— see `--include-tests`/`--include-generated` below for what changes when a
diff does touch test symbols or generated files.

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

The line under the heading summarizes the shape of the change — how many
symbols changed, across how many files, and (for multi-file diffs) which
file concentrates most of them, e.g.
`16 changed symbols in 3 files — most in store/items.go (11)` — so a
reviewer sees the epicenter before reading a single tree line.

"Change graph" reads top-down in call-hierarchy order: `main` is the only
entry point (nothing else changed in this diff calls it), and every
function it reaches is nested beneath it. `build_resolver` is reachable
from both `main` and `run_base_pipeline`; it is rendered in full once,
under `main` (the first place it's reached), and referenced by name only
(`(see above)`) the second time — so a reviewer never sees the same
signature twice. When two changed symbols depend on each other (a
mutual-recursion cycle), the edge that closes the loop is marked
`⚠️ ... — dependency cycle, see above` instead of being walked into again;
see [ADR 0008](docs/adr/0008-entry-point-tree-rendering-for-changed-symbols.md)
for the full rationale.

Two more condensations
([ADR 0012](docs/adr/0012-condense-change-graph-rendering.md)) keep
request/response-style diffs readable. Changed **data-carrier types**
(structs/enums/type aliases with no outgoing edges of their own) don't get
tree lines; they're folded into the line of each symbol that references
them as a `— uses:` annotation, with their full signatures still listed
under "Definitions". And an **interface/trait declaration is linked to its
changed methods** by method-spec name, so the methods nest under the
interface instead of duplicating it at top level. A Go diff adding an
`ItemStore` interface, two receiver-method implementations, and four
request/response structs — 8 changed symbols — renders as just three tree
lines:

```markdown
## Change graph

8 changed symbols in 1 file

- interface ItemStore (store.go) — uses: ListItemsRequest, ListItemsResponse, SaveItemRequest, SaveItemResponse
  - fn ListItems (store.go) — uses: ListItemsRequest, ListItemsResponse, itemStore
  - fn SaveItem (store.go) — uses: SaveItemRequest, SaveItemResponse, itemStore
```

(The "Definitions" section, omitted here, still carries all 8 full
signatures.)

Unchanged 1-hop dependencies (ADR 0003) — functions/types the diff touches
but did not itself change — still show up as a `Depends on:` list under
each definition, same as before ADR 0008; they're omitted from the example
above (`--deps 0`) to keep it focused on the tree shape.

`--format json` renders the same data as structured JSON instead
(`{"files": [...], "skipped": [...], "graph": {"nodes", "edges", "roots"}}`),
for piping into `jq` or another tool. The `graph` field is the same
call-graph data "Change graph" renders as a tree, so JSON consumers don't
need to recompute it from `referenced_names`:

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

Each symbol in `files[].symbols` also carries an `id` field matching its
`graph` node's `id`, so a consumer can join a symbol's full signature back
to its position in the graph without recomputing the `{path}::{name}` id
scheme itself.

The top-level JSON report also carries `"tests": [{"path", "symbol_count"}]`
(ADR 0009's per-file test-symbol counts, empty unless the diff touched any)
and `skipped[].reason` can be `"generated"` (ADR 0010 attribute-based
detection, or ADR 0011 content-marker detection — both report the same
reason value) alongside the existing
`"unsupported_language"`/`"binary"`/`"deleted"` values.

### When same-name matches are capped

On a repository with many same-named definitions, the same-name cap (see
Known limitations below) shows up directly in the output. Running
`rinkaku` against [astral-sh/ruff](https://github.com/astral-sh/ruff)
commit `6237ecb4d` ("[ty] Add progress reporting to workspace
diagnostics"), a changed `LazyWorkDoneProgress` struct references `Inner`,
a name defined 14 times across the repo (mostly unrelated Python test
fixtures and formatter cases named `class Inner`) — the 3 closest matches
by path proximity are shown, and the rest are reported as a count instead
of listed in full or silently dropped:

````markdown
### struct LazyWorkDoneProgress (crates/ty_server/src/server/lazy_work_done_progress.rs)

```
pub(super) struct LazyWorkDoneProgress { inner: Arc<Inner>, }
```

Depends on:
- `crates/ruff_linter/resources/test/fixtures/pyupgrade/UP046_0.py`: `class Inner(Generic[T]): var: T`
- `crates/ruff_python_formatter/resources/test/fixtures/black/cases/class_methods_new_line.py`: `class Inner: pass`
- `crates/ruff_python_formatter/resources/test/fixtures/black/cases/class_methods_new_line.py`: `class Inner: """Just a docstring.""" def __init__(self):`
- (+11 more definitions matched by name)
````

This same 810-line diff also shows the noise reduction from filtering `_`
and single-character identifiers out of referenced names entirely (see
Known limitations): the pre-#9 QA pass on this exact diff found 76 of 188
"Depends on" lines were unrelated `def _(...)` matches (~40% noise); on
the current `main`, the same diff produces 384 output lines (up from 295
pre-ADR-0008 due to the added "Change graph" tree section, and down from
405 pre-#9) with zero `_`-related entries, since `_` is no longer looked
up at all.

### `--deps`

`--deps 1` (the default) resolves each changed symbol's 1-hop dependencies
by indexing every file `git ls-files` tracks in the repository — this
makes the output more useful (you see what a changed function calls) but
costs an up-front repo-wide scan. `TagsResolver::new` prefilters which
files are worth parsing (skipping any file whose content cannot contain
any referenced name at all), which helps when referenced names are
distinctive but has limited effect when they include common
standard-library-style names (see Known limitations). On the ruff
`6237ecb4d` diff above, `--deps 1` took ~6.5s post-#9 (down from ~9.5s
pre-#9) versus ~0.05s for `--deps 0` on the same diff — `--deps 0` skips
resolution entirely (no "Depends on" sections, no repository scan) and
remains dramatically faster since indexing cost does not depend on diff
size. Prefer `--deps 0` for quick iteration or CI checks where the
dependency context isn't needed.

### `--include-tests`

By default (see [ADR 0009](docs/adr/0009-exclude-test-symbols-from-output-by-default.md)),
test symbols are excluded from "Change graph"/"Definitions" — detected per
language (Go's `*_test.go`, Python's `test_*.py`/`*_test.py` and `tests/`
directories, TypeScript's `*.test.ts(x)`/`*.spec.ts(x)` and `__tests__/`,
and Rust's `tests/` directory plus `#[cfg(test)]` modules and
`#[test]`/`#[rstest]`/`#[tokio::test]`-attributed functions) — and
summarized instead as a per-file count under a `## Tests` section:

```markdown
## Tests

- rinkaku-core/src/pipeline.rs: 3 changed test symbols
```

This keeps the primary output focused on implementation entry points while
still surfacing "did this change come with tests?" as a signal. Pass
`--include-tests` to restore the previous behavior (test symbols appear in
the graph and definitions like any other symbol, and `## Tests` is omitted).

### `--include-generated`

By default, rinkaku skips generated files two ways, both controlled by this
one flag:

- **`.gitattributes`** (see [ADR 0010](docs/adr/0010-skip-files-marked-no-diff-or-generated-in-gitattributes.md)):
  resolves each changed file's `diff`/`linguist-generated` attributes via
  `git check-attr` and skips files marked `-diff` or `linguist-generated`.
  Only applies when a local git repository is available (`--base`, `--pr`,
  or stdin piped inside a repository); outside a repository, this check
  does not run.
- **Content markers** (see [ADR 0011](docs/adr/0011-detect-generated-files-by-content-markers.md)):
  independent of `.gitattributes`, and doesn't require a git repository —
  skips a file whose first 5 lines contain a linguist-compatible marker: a
  `@generated` comment, or a line with both `Code generated` and
  `DO NOT EDIT` (Go tooling/protobuf's
  `// Code generated by <tool>. DO NOT EDIT.` convention, matched
  regardless of the comment syntax a language uses). This is what catches
  generated code in repositories that never configured `.gitattributes` at
  all and rely on GitHub linguist's own content-based detection instead.

Either way, skipped files (lockfiles, generated code — content already
declared uninteresting to diff-review) are dropped from Markdown output
silently: they do **not** appear under "Skipped files" at all, since
listing them as something rinkaku "didn't look at" would just be noise on
top of what's already known about them. A diff that touches nothing but
generated files renders as empty Markdown output.

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
files — by either detection method — are analyzed like any other file, in
both output formats).

### `--entry <path>`

By default, "Change graph"/"Repository graph" is rooted at auto-detected
entry points (ADR 0008): symbols nothing else in the graph depends on.
`--entry <path>` re-roots the tree at a chosen path instead (ADR 0019) —
entry points become the symbols under `path` that nothing else *under that
same path* depends on, and dependency trees still expand outward through
the whole graph as usual. This is a change of vantage point, not a filter:
symbols outside `path` are neither hidden nor excluded from analysis, only
no longer eligible to be roots themselves. Useful for carving a reviewable
viewpoint out of a large or whole-repo graph — e.g. "what does this change
look like from the API layer":

```sh
rinkaku --base main --entry src/api
```

Works with every input mode (stdin / `--base` / `--pr` / whole-repo) and
combines with `--tui`: the TUI opens with the cursor already on the tree
row matching `path` and the right-hand pane already in Pivot mode (the `p`
pivot below) — the interactive session starts exactly where `--entry`
would have rooted the Markdown/JSON tree, rather than requiring you to
locate the row and press `p` yourself. If no tree row's path matches `path`
exactly, the TUI opens normally (cursor on the first row, Detail pane) with
a status-line note instead.

Prints `note: no symbols under <path>` to stderr and renders an empty tree
when no symbol's path falls under `path`. Fan-in counts (Hotspots, and the
TUI tree's `^N` badge) stay whole-analysis under `--entry`/the TUI pivot —
a pivot changes the vantage point (which symbols count as entry points),
not the direction fan-in itself measures, so scoping it to the pivoted
subset would misreport how much the rest of the repository actually
depends on a symbol (see [ADR 0019](docs/adr/0019-entry-path-pivot-view.md)'s
Consequences).

## Interactive TUI

Markdown/JSON stay optimized for LLMs and CI ([ADR 0015](docs/adr/0015-tui-for-humans-markdown-for-machines.md));
for a human reviewing a change in a terminal, pass `--tui` instead of
`--format` to open an interactive terminal UI ([ADR 0016](docs/adr/0016-tui-crate-and-stack.md)),
built with [ratatui](https://ratatui.rs):

```sh
rinkaku --base main --tui
```

`--tui` takes the same input flow as every other mode (stdin / `--base` /
`--pr`) and only changes the output stage, so it conflicts with `--format`
rather than combining with it. Bare `rinkaku`, run on an interactive
terminal with no `--base`/`--pr`, opens the TUI automatically on a
whole-repo outline instead of a diff ([ADR 0017](docs/adr/0017-whole-repo-outline-as-default-input-mode.md));
its diff pane (`d`) has nothing to show in that case and renders a
placeholder.

### What it shows

- **Entry pane (left):** the directory tree of changed files, not the
  call-graph tree — nesting depth mirrors your repository's layout.
  Directories and files carry badges: `~N` changed symbols, `!N` contract
  changes (added/removed/signature-changed), `^N` fan-in (hotspot
  aggregate). A directory that participates in a dependency cycle is
  marked `(cycle)`. By default, sibling directories are ordered
  topologically — entry points (least depended-on) first, foundations
  (most depended-on) last — the same shape the "Change graph" root-finding
  uses in Markdown, condensed to the directory level; `o` toggles to plain
  alphabetical order. Symbol rows show a kind abbreviation (`fn`, `struct`,
  ...) and a classification marker: `+` added, `~` signature-changed, `x`
  removed (dimmed and crossed out).
- **Detail pane (right):** what the cursor is on. A symbol row shows its
  classification, signature (an old/new diff when the contract changed),
  who depends on it ("used by"), and its callees. A file row shows every
  symbol changed in that file with its classification marker and fan-in. A
  directory row shows its badge breakdown and top fan-in symbols, plus —
  when it participates in a dependency cycle — exactly which other
  directories it cycles with and the concrete symbol-to-symbol edges
  forming that cycle.
- **Diff pane (right):** `d`/`D` toggles the right-hand pane to the raw
  unified-diff hunks instead of the detail view — every hunk of the file
  for a file row, or just the hunks intersecting a symbol's own line range
  for a symbol row (a directory row has no single diff to show, since it
  spans multiple files). Hunks in the four built-in languages are
  syntax-highlighted (keywords, strings, types, ...); added/removed lines
  keep their green/red diff signal as a background tint so it doesn't
  compete with token colors, and any other file falls back to plain
  green/red text.
- **Pivot pane (right):** `p`/`P` toggles the right-hand pane to an
  entry-tree view rooted at the directory or file row under the cursor
  ([ADR 0019](docs/adr/0019-entry-path-pivot-view.md)) — the interactive
  equivalent of `--entry <path>`. The tree follows the cursor: move to a
  different directory/file row while pivoted and it re-renders rooted at
  the new row's path; a symbol row shows a placeholder instead, since
  pivoting only makes sense on a directory/file scope. Nodes reached only
  by expanding a dependency edge outward past the pivoted path are dimmed
  so you can tell "the layer I pivoted on" from "what it reaches into".
  Press `p` again, or `d`, to leave pivot mode.
- **Scrolling the right-hand pane:** `J`/`K` scroll the Detail/Diff/Pivot
  pane down/up by one line when its content is too long to fit — the
  pane's title grows a `(first-last/total)` suffix (e.g. `Detail
  (1-17/43)`) whenever there's more to see, so a long cycle-edge list or a
  large file's diff doesn't quietly get cut off. The scroll position
  resets to the top whenever the underlying content could have changed:
  moving the cursor, toggling between the detail/diff/pivot views, or
  returning from the source view.
- **Source view:** `s` on a symbol row opens that file, scrolled to and
  highlighting the symbol's line range; `esc`/`q` returns to the entry
  view. Reads the working tree directly (not the historical commit a
  `--base`/`--pr` diff was computed against), so it always shows the
  file's current content — note that the highlighted line range itself is
  from analysis time, so it can drift (or, if the file has since shrunk,
  get clamped to the end of the file) if you edit the file after opening
  the TUI.

### Key bindings

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Move the cursor |
| `enter` / `space` | Expand or collapse a directory/file row |
| `e` / `E` | Expand every row |
| `c` / `C` | Collapse every row |
| `o` / `O` | Toggle topological / alphabetical ordering |
| `d` / `D` | Toggle the right-hand pane between detail and diff |
| `p` / `P` | Toggle the right-hand pane to the pivot tree rooted at the selected directory/file |
| `J` / `K` | Scroll the right-hand pane (Detail/Diff/Pivot) down/up |
| `s` / `S` | Open the source view for the symbol under the cursor |
| `esc` / `q` | Return to the entry view (from the source view) |
| `q` / `ctrl-c` | Quit (from the entry view) |

Glyphs are plain ASCII (`~`/`!`/`^`/`+`/`x`, `v`/`>` for expand state)
rather than Unicode/emoji, for compatibility with plainer terminal
configurations.

## Using rinkaku with LLM reviewers

rinkaku's output works well as a "map" handed to an LLM before it reads a
diff: the hotspots, contract markers (added/removed/signature-changed),
and entry-point trees let the reviewer allocate deep-reading attention
instead of reconstructing the change's structure itself.

1. Generate the map from a **trusted checkout** — a clean `main` build,
   never the branch under review, so a malicious or buggy diff can't tamper
   with the tool inspecting it:

   ```sh
   rinkaku --pr 123 --format md > map.md
   # or: rinkaku --base main --format md > map.md
   ```

2. Paste `map.md` at the top of the reviewer's prompt, followed by the
   actual diff, with instructions along these lines:

   ```
   Here is a structural map of this change (hotspots, contract markers,
   entry-point trees). Use it to decide where to read deeply first, but
   it is an attention-allocation aid, not a verifier: read the full
   implementation of anything it flags, and don't assume unflagged code
   is safe to skip. Then review the diff below.
   ```

3. Run an **independent pass without the map** alongside the map-assisted
   one. Across repeated trials, the two passes consistently surface
   different findings rather than one being a superset of the other — see
   [`docs/experiments/0001-map-assisted-llm-review.md`](docs/experiments/0001-map-assisted-llm-review.md).

4. Whichever pass(es) you run, always add a **dynamic verification**
   step: build and actually execute the changed code, including
   failure-mode invocations (non-TTY stdin, empty input, missing files).
   Behavioral bugs don't show up on the signature surface the map draws
   from, and the experiment's own rounds found real defects (a non-TTY
   panic) only by running the binary.

## GitHub Action

This repository ships a composite [`action.yaml`](action.yaml) that runs
rinkaku against a pull request's diff and posts (or updates) a sticky PR
comment: a [`--format mermaid`](#usage) call/dependency graph up front —
rendered natively by GitHub in the comment — with the full Markdown
outline collapsed underneath for anyone who wants signature-level detail.

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

Inputs: `version` (a release tag to download, default `latest`), `binary`
(path to an already-built rinkaku binary, skipping the download entirely —
see this repository's own [dogfooding workflow](.github/workflows/rinkaku-report.yaml),
which builds rinkaku from the PR's *base* ref rather than trusting a
downloaded binary, for the same reason the LLM-review recipe above always
builds its map from a trusted checkout), `base` (defaults to the PR's base
ref), `github-token` (defaults to `github.token`), and `comment` (set
`false` to skip posting and just get the `mermaid-path`/`markdown-path`
outputs).

## Development

Requires a Rust toolchain (pinned in [`rust-toolchain.toml`](rust-toolchain.toml);
`rustup` will install it automatically).

The workspace has three crates: `rinkaku-core` (the pure diff-condensation
library, published standalone so it can be embedded in other tools),
`rinkaku-tui` (the interactive terminal UI's view-models and `ratatui`
rendering, depending on `rinkaku-core`; see [ADR 0016](docs/adr/0016-tui-crate-and-stack.md)),
and `rinkaku` (the thin CLI binary, depending on both).

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

### Known limitations

**Mitigated in [#9](https://github.com/hiro-o918/rinkaku/pull/9):** the
original QA pass (see below) found name-only matching noise and slow
`--deps 1` indexing severe enough to block merging. Both are improved,
though not eliminated — v1's resolver is still name-only (see "still
open" below).

- **Same-name matches are ranked and capped, not resolved.** When several
  definitions share a referenced name, they are ranked by path proximity
  to the referencing file (same file > same directory > shared path
  prefix depth > other) and only the top 3 ([`MAX_MATCHES_PER_NAME`]) are
  shown; the rest are reported as a count (`(+N more definitions matched
  by name)` in Markdown, `omitted_matches` in JSON) rather than silently
  dropped or listed in full — see "When same-name matches are capped"
  above for a real example. This bounds "Depends on" noise but does not
  guarantee the top 3 include the actually-referenced definition, since
  ranking is a proximity heuristic, not type-aware resolution.
- **`_` and single-character identifiers are never resolved.** They are
  filtered out of referenced names entirely, since under name-only
  resolution they match too many unrelated definitions to be useful
  (Python's `_` placeholder convention was the main offender found in
  QA — see below).
- **The `--deps 1` indexing prefilter has limited effect when a diff
  references common standard-library-style names.** `TagsResolver::new`
  skips parsing files whose content cannot contain any referenced name at
  all (measured ~88% fewer files parsed, ~8x faster indexing on a
  same-language-only reference set — see PR #9's description for the full
  numbers). But a name like `Vec`, `Option`, `String`, `Some`, or `Ok`
  appears in nearly every Rust file in a real codebase, so a diff whose
  referenced names include several of these sees a smaller reduction (on
  the ruff `6237ecb4d` diff used above, `--deps 1` dropped from ~9.5s
  pre-#9 to ~6.5s post-#9 — better, not solved). The prefilter is a
  substring match over raw file content, not scoped to actual
  definitions, so it cannot distinguish "defines `Vec`" from "mentions
  `Vec`" without also risking false negatives (see `deps.rs`'s
  `should_parse_file` doc comment) — narrowing this further is left for a
  future iteration. The dominant cost in `--base` mode remains the
  per-file `git show` subprocess invocation for reading tracked files
  (unrelated to this prefilter, and unaddressed — see `deps.rs`'s
  performance doc comment).

**Still open — no type resolution (by design, ADR 0003):** dependency
resolution matches referenced names against definitions by name alone,
with no type information — it cannot disambiguate overloads, shadowed
names, or same-named symbols in unrelated modules. The ranking and cap
above reduce the resulting noise but do not fix the underlying
imprecision (e.g. an unrelated same-named Python test fixture class can
still outrank a real dependency once same-file/same-directory candidates
are exhausted — see the `Inner` example above). A future `Resolver`
implementation backed by an LSP server (pyright, gopls, rust-analyzer,
...) is planned as a higher-precision, opt-in alternative for v2+; see
the Roadmap below.

[`MAX_MATCHES_PER_NAME`]: rinkaku-core/src/deps.rs

### Roadmap / not yet done

- LSP-backed `Resolver` implementations (pyright, gopls, rust-analyzer,
  ...) as a higher-precision, opt-in alternative to the v1 tags-based
  `Resolver`.

## Release

`rinkaku` and `rinkaku-core` are versioned independently by
[release-please](https://github.com/googleapis/release-please) (no
`linked-versions` grouping): each crate only bumps when a commit touches
its own path, so it's normal for them to be on different versions (e.g.
`rinkaku` 0.2.0 depending on `rinkaku-core` 0.1.0). Only `rinkaku`'s
release tag (`v{version}`, no component prefix) triggers
`build-and-publish.yaml`; `rinkaku-core`'s tag is prefixed
(`rinkaku-core-v{version}`) so a `rinkaku-core`-only release doesn't spin
up the binary build/publish pipeline.

`separate-pull-requests: true` is set for a reason that isn't obvious
from the config alone: with more than one non-root `packages` entry (no
`.` path), release-please's PR-merging step can't find a "root" release
candidate to base the combined PR's title on, and falls back to a title
that omits the version entirely (`chore: release main`). That title
doesn't match what the *next* run expects when looking up the
already-merged PR to tag, so tagging silently finds nothing to do and
`release-main.yaml` aborts with "untagged, merged release PRs
outstanding" -- this bit us for both the v0.2.0 and v0.3.0 releases,
each requiring a manual `gh release create` + relabeling the PR
`autorelease: tagged` to recover. `separate-pull-requests: true` sidesteps
this entirely: each package gets its own PR (and its own title, correctly
including that package's version), so there's no combined-PR title to
compute in the first place.

## License

MIT
