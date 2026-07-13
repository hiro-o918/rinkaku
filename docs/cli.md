# CLI reference

The [TUI](tui.md) and [GitHub Action](action.md) are the recommended
entry points, but the same analysis is available as plain CLI output —
useful for scripting, CI gates, custom tooling, or feeding downstream
processors.

## Input modes

| Mode | Reads files from | Notes |
| --- | --- | --- |
| **stdin** — `gh pr diff 123 \| rinkaku` | working tree | Assumes the piped diff matches the working tree. A stale/unrelated piped diff will misalign line numbers. |
| **`--base <ref>`** — `rinkaku --base main` | `git show <head>:<path>` | Runs `git diff` internally; extraction always matches the diffed commit. |
| **`--pr <number>`** — inside a local clone | `git show`-backed | Requires `gh` installed and authenticated ([ADR 0004](adr/0004-pr-input-mode-via-gh-in-local-clone.md)). |
| **`--pr <url>`** — from any directory | `git show`-backed | Prefers an existing [ghq](https://github.com/x-motemen/ghq) clone; otherwise auto-clones blobless into a cache ([ADR 0005](adr/0005-auto-clone-into-cache-for-pr-urls.md) / [ADR 0006](adr/0006-prefer-ghq-managed-clones-over-cache.md)). Private repos need `gh auth setup-git`. |
| **Whole-repo** — bare `rinkaku` | working tree | No diff involved; outlines every symbol and its dependency structure ([ADR 0017](adr/0017-whole-repo-outline-as-default-input-mode.md)). Default on a TTY is the TUI. |

## Output formats (`--format`)

```sh
rinkaku --base main --format md        # Markdown (default when stdout is not a TTY)
rinkaku --base main --format json      # Structured JSON for tooling
rinkaku --base main --format mermaid   # A flowchart for pasting into a PR comment
```

`--tui` replaces the output stage entirely and conflicts with
`--format`.

## Markdown example

Running rinkaku on
[commit aa7ca34](https://github.com/hiro-o918/rinkaku/commit/aa7ca34)
(a 35-line diff adding stderr progress logging to `main.rs`):

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

...
````

The line under the heading summarizes the shape of the change (symbol
count, file count, and — for multi-file diffs — the file with the
epicenter, e.g. `16 changed symbols in 3 files — most in
store/items.go (11)`).

**"Change graph"** reads top-down in call-hierarchy order
([ADR 0008](adr/0008-entry-point-tree-rendering-for-changed-symbols.md)):
entry points at the top, callees nested. A symbol reachable from more
than one place is rendered fully once and marked `(see above)` on
repeats. Mutual-recursion cycles are marked
`⚠️ ... — dependency cycle, see above`.

Two condensations
([ADR 0012](adr/0012-condense-change-graph-rendering.md)) keep
request/response-style diffs readable: **data-carrier types** (structs
/ enums / type aliases with no outgoing edges) fold into `— uses:`
annotations on their referencer's line, and **interface/trait methods
nest under their declaration** by method-spec name.

Unchanged 1-hop dependencies (ADR 0003) show up as a `Depends on:` list
under each definition.

## JSON output

`--format json` renders the same data as
`{"files": [...], "skipped": [...], "graph": {"nodes", "edges", "roots"}}`,
plus a `"tests": [{"path", "symbol_count"}]` array (ADR 0009's per-file
test-symbol counts, non-empty only under `--exclude-tests` or when the
diff touched test symbols under the default).

The `graph` field is the same call-graph "Change graph" renders as a
tree, so JSON consumers don't need to recompute it. Each entry in
`files[].symbols` carries an `id` matching its `graph` node's `id`.

`skipped[].reason` can be `"generated"` (ADR 0010 attribute-based
detection, or ADR 0011 content-marker detection — both report the same
reason) alongside `"unsupported_language"` / `"binary"` / `"deleted"`.

## `--format mermaid`

Emits the same call/dependency graph as a mermaid flowchart
([ADR 0021](adr/0021-mermaid-output-format.md)), designed for pasting
into a GitHub PR comment or description where mermaid renders
natively. This is the format the [GitHub Action](action.md) uses for
the top of its sticky comment.

## Flags

### `--deps <N>`

`--deps 1` (default) resolves each changed symbol's 1-hop
dependencies by indexing every file `git ls-files` tracks — richer
output at the cost of an up-front repo-wide scan. `--deps 0` skips
resolution entirely (no `Depends on:` sections, no repo scan) and is
dramatically faster; use it for quick iteration or CI checks that
don't need dependency context. See
[Known limitations](#known-limitations) for precision caveats.

### `--exclude-tests`

By default ([ADR 0025](adr/0025-default-to-including-tests.md)), test
symbols appear alongside production symbols and the `## Tests` section
is omitted — this shape is designed for LLM consumers, for which
"which contracts have which tests changed alongside them" is signal
rather than noise.

Pass `--exclude-tests` to opt into the previous behavior: test symbols
(Go's `*_test.go`, Python's `test_*.py`/`*_test.py` and `tests/`,
TypeScript's `*.test.ts(x)`/`*.spec.ts(x)` and `__tests__/`, Rust's
`tests/` plus `#[cfg(test)]` and `#[test]`/`#[rstest]`/`#[tokio::test]`)
are summarized as per-file counts under `## Tests` instead, and
`TagsResolver`'s dependency index applies the same exclusion (ADR
0009).

### `--include-generated`

By default, rinkaku skips generated files two ways:

- **`.gitattributes`** ([ADR 0010](adr/0010-skip-files-marked-no-diff-or-generated-in-gitattributes.md)):
  skips files marked `-diff` or `linguist-generated` (via
  `git check-attr`; only when a git repository is available).
- **Content markers** ([ADR 0011](adr/0011-detect-generated-files-by-content-markers.md)):
  skips a file whose first 5 lines contain a linguist-compatible
  marker (`@generated`, or a `Code generated ... DO NOT EDIT` line
  regardless of comment syntax).

Skipped generated files are dropped from Markdown silently (a diff
that touches only generated files renders as empty Markdown) but do
appear in `--format json`'s `skipped` array with reason `"generated"`.

Pass `--include-generated` to disable both detections.

### `--entry <path>`

Re-roots "Change graph"/"Repository graph" at `path`
([ADR 0019](adr/0019-entry-path-pivot-view.md)) — entry points become
the symbols under `path` that nothing else *under that same path*
depends on. Symbols outside `path` are neither hidden nor excluded
from analysis, only no longer eligible to be roots.

```sh
rinkaku --base main --entry src/api
```

Combines with `--tui`: opens with the cursor on the row matching
`path` and the Blast-radius pane already active. Prints
`note: no symbols under <path>` to stderr and renders an empty tree
when nothing matches. Fan-in counts remain whole-analysis (see
[ADR 0019](adr/0019-entry-path-pivot-view.md)'s Consequences).

## `self-update`

```sh
rinkaku self-update            # prompts before installing
rinkaku self-update --yes      # non-interactive
```

Downloads the latest release for your platform and replaces the
running binary in place. If you installed via Homebrew or
`cargo install`, prefer `brew upgrade` / `cargo install rinkaku` so
your package manager's bookkeeping stays in sync. When stdin is not a
terminal and `--yes` is not given, `self-update` refuses to run rather
than silently proceeding.

## Known limitations

**Mitigated in [#9](https://github.com/hiro-o918/rinkaku/pull/9):** the
original QA pass found name-only matching noise and slow `--deps 1`
indexing severe enough to block merging. Both are improved, though
not eliminated — v1's resolver is still name-only.

- **Same-name matches are ranked and capped, not resolved.** When
  several definitions share a referenced name, they are ranked by path
  proximity (same file > same directory > shared prefix depth > other)
  and only the top 3 ([`MAX_MATCHES_PER_NAME`]) are shown; the rest
  are reported as a count. Bounds noise but doesn't guarantee the top
  3 include the actually-referenced definition — ranking is a
  proximity heuristic, not type-aware resolution.
- **`_` and single-character identifiers are never resolved.**
  Filtered out entirely; under name-only resolution they match too
  many unrelated definitions to be useful.
- **The `--deps 1` indexing prefilter has limited effect on
  standard-library-style names.** `TagsResolver::new` skips parsing
  files whose content cannot contain any referenced name at all
  (measured ~88% fewer files parsed, ~8x faster indexing on a
  same-language-only reference set — see PR #9). But a name like
  `Vec`, `Option`, `String`, `Some`, or `Ok` appears in nearly every
  Rust file, so a diff referencing several of these sees a smaller
  reduction. The dominant cost in `--base` mode remains the per-file
  `git show` subprocess for reading tracked files (unaddressed).

**Still open — no type resolution (by design, ADR 0003):** dependency
resolution matches referenced names against definitions by name alone
— it cannot disambiguate overloads, shadowed names, or same-named
symbols in unrelated modules. A future `Resolver` backed by an LSP
server (pyright, gopls, rust-analyzer, ...) is planned as a
higher-precision, opt-in alternative for v2+.

[`MAX_MATCHES_PER_NAME`]: ../rinkaku-core/src/deps.rs
