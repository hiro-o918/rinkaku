# CLI usage and output format

This page covers every rinkaku invocation mode, its output formats
(Markdown/JSON/mermaid), and the flags that shape what gets included. For a
first invocation, see the [README's Quick start](../README.md#quick-start).
For the interactive terminal UI, see
[The interactive TUI](tui.md). For the composite GitHub Action, see
[Using the GitHub Action from another repository](github-action.md).

## Input modes

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
# 0021) — opt-in, meant for pasting into a GitHub PR comment/description
# where mermaid renders natively, not for piping into an LLM
rinkaku --base main --format mermaid

# Skip dependency resolution (faster, no repo-wide index — see below)
rinkaku --base main --deps 0
```

- **Input**: a unified diff via stdin (`gh pr diff 123 | rinkaku`),
  `rinkaku --base main` to run `git diff` internally, or
  `rinkaku --pr 123` to review a GitHub PR directly. In `--base` mode,
  file contents are read via `git show <head>:<path>` so extraction always
  matches the diffed commit, regardless of the working tree's state.
  `--pr` mode resolves the PR's base and head via `gh pr view`, fetches
  both, and reuses the same `git show`-backed read strategy as `--base`
  (see [ADR 0004](adr/0004-pr-input-mode-via-gh-in-local-clone.md)).
  A bare PR number requires running inside a local clone of the target
  repository; a URL also works from any directory, preferring an existing
  [ghq](https://github.com/x-motemen/ghq)-managed clone when one matches
  and otherwise auto-cloning a blobless copy into a cache (see
  [ADR 0005](adr/0005-auto-clone-into-cache-for-pr-urls.md) and
  [ADR 0006](adr/0006-prefer-ghq-managed-clones-over-cache.md)). `gh`
  must be installed and authenticated either way. In
  stdin mode, file contents are read off the working tree, which assumes
  **the piped diff is consistent with the current working tree** (e.g. it
  was just produced by `git diff` or already applied); a stale or
  unrelated diff piped via stdin can produce misaligned line numbers.

## What it looks like

All examples below are captured from the `cargo build --release` binary
post-[ADR 0008](adr/0008-entry-point-tree-rendering-for-changed-symbols.md)
(entry-point tree rendering) and
[ADR 0012](adr/0012-condense-change-graph-rendering.md)
(condensed rendering), not fabricated. The example diff below
touches no test symbols and no generated files, so it renders the same
with or without the defaults introduced in
[ADR 0009](adr/0009-exclude-test-symbols-from-output-by-default.md),
[ADR 0010](adr/0010-skip-files-marked-no-diff-or-generated-in-gitattributes.md),
and [ADR 0011](adr/0011-detect-generated-files-by-content-markers.md)
— see [`--include-tests`/`--include-generated`](#--include-tests) below
for what changes when a diff does touch test symbols or generated files.

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
see [ADR 0008](adr/0008-entry-point-tree-rendering-for-changed-symbols.md)
for the full rationale.

Two more condensations
([ADR 0012](adr/0012-condense-change-graph-rendering.md)) keep
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

## JSON output

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

## When same-name matches are capped

On a repository with many same-named definitions, the same-name cap (see
[Known limitations](architecture-and-limitations.md#known-limitations))
shows up directly in the output. Running
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
[Known limitations](architecture-and-limitations.md#known-limitations)):
the pre-#9 QA pass on this exact diff found 76 of 188
"Depends on" lines were unrelated `def _(...)` matches (~40% noise); on
the current `main`, the same diff produces 384 output lines (up from 295
pre-ADR-0008 due to the added "Change graph" tree section, and down from
405 pre-#9) with zero `_`-related entries, since `_` is no longer looked
up at all.

## Flags

### `--deps`

`--deps 1` (the default) resolves each changed symbol's 1-hop dependencies
by indexing every file `git ls-files` tracks in the repository — this
makes the output more useful (you see what a changed function calls) but
costs an up-front repo-wide scan. `TagsResolver::new` prefilters which
files are worth parsing (skipping any file whose content cannot contain
any referenced name at all), which helps when referenced names are
distinctive but has limited effect when they include common
standard-library-style names (see
[Known limitations](architecture-and-limitations.md#known-limitations)).
On the ruff `6237ecb4d` diff above, `--deps 1` took ~6.5s post-#9 (down
from ~9.5s pre-#9) versus ~0.05s for `--deps 0` on the same diff —
`--deps 0` skips resolution entirely (no "Depends on" sections, no
repository scan) and remains dramatically faster since indexing cost does
not depend on diff size. Prefer `--deps 0` for quick iteration or CI
checks where the dependency context isn't needed.

### `--include-tests`

By default (see [ADR 0009](adr/0009-exclude-test-symbols-from-output-by-default.md)),
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

- **`.gitattributes`** (see [ADR 0010](adr/0010-skip-files-marked-no-diff-or-generated-in-gitattributes.md)):
  resolves each changed file's `diff`/`linguist-generated` attributes via
  `git check-attr` and skips files marked `-diff` or `linguist-generated`.
  Only applies when a local git repository is available (`--base`, `--pr`,
  or stdin piped inside a repository); outside a repository, this check
  does not run.
- **Content markers** (see [ADR 0011](adr/0011-detect-generated-files-by-content-markers.md)):
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
row matching `path` and the right-hand pane already in Pivot mode (see
[The interactive TUI](tui.md#what-it-shows)) — the interactive session
starts exactly where `--entry` would have rooted the Markdown/JSON tree,
rather than requiring you to locate the row and press `p` yourself. If no
tree row's path matches `path` exactly, the TUI opens normally (cursor on
the first row, Detail pane) with a status-line note instead.

Prints `note: no symbols under <path>` to stderr and renders an empty tree
when no symbol's path falls under `path`. Fan-in counts (Hotspots, and the
TUI tree's `^N` badge) stay whole-analysis under `--entry`/the TUI pivot —
a pivot changes the vantage point (which symbols count as entry points),
not the direction fan-in itself measures, so scoping it to the pivoted
subset would misreport how much the rest of the repository actually
depends on a symbol (see [ADR 0019](adr/0019-entry-path-pivot-view.md)'s
Consequences).
