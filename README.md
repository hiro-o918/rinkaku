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
  repository; a URL also works from any directory by auto-cloning a
  blobless copy into a cache (see
  [ADR 0005](docs/adr/0005-auto-clone-into-cache-for-pr-urls.md)). `gh`
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
# From a GitHub PR (stdin, no local clone required)
gh pr diff 123 | rinkaku

# From a GitHub PR by number: run inside a local clone of the target
# repository (fetches the PR via `gh`/`git`; requires `gh` installed and
# authenticated)
rinkaku --pr 123

# From a GitHub PR URL: works from any directory. If the cwd isn't already
# a clone of that repository, rinkaku auto-clones a blobless copy into
# $RINKAKU_CACHE_DIR / $XDG_CACHE_HOME/rinkaku / ~/.cache/rinkaku and runs
# there instead (see ADR 0005). Private repos need `gh auth setup-git` so
# the cache clone's later `git fetch`s can authenticate too.
rinkaku --pr https://github.com/octocat/hello-world/pull/123

# From a local git diff against a base branch
rinkaku --base main

# JSON output for feeding into another tool or LLM
rinkaku --base main --format json

# Skip dependency resolution (faster, no repo-wide index — see below)
rinkaku --base main --deps 0
```

### What it looks like

All examples below are captured from the `cargo build --release` binary
against `main` post-[#9](https://github.com/hiro-o918/rinkaku/pull/9)
(the dependency-resolution precision and indexing performance
improvements), not fabricated.

Running `rinkaku` on
[a real rinkaku commit](https://github.com/hiro-o918/rinkaku/commit/1ccc183)
(a 128-line diff adding the stdin garbage-input warning in `main.rs`)
produces:

```sh
$ git show 1ccc183 --format="" | rinkaku
```

````markdown
## rinkaku-core/src/main.rs

```
fn main() -> anyhow::Result<()>
```

Depends on:
- `rinkaku-core/src/deps.rs`: `pub trait Resolver { fn resolve(&self, name: &str) -> Vec<ResolvedSymbol>; }`
- `rinkaku-core/src/pipeline.rs`: `pub fn analyze_diff( diff_text: &str, read_file: impl Fn(&str) -> std::io::Result<String>, resolver: Option<&dyn Resolver>, ) -> Result<Report, AnalyzeError>`
- `rinkaku-core/src/main.rs`: `fn build_resolver( cli: &Cli, diff_text: &str, diff_read_file: impl Fn(&str) -> std::io::Result<String>, head: Option<&str>, cwd: Option<&std::path::Path>, ) -> anyhow::Result<Option<TagsResolver>>`
- `rinkaku-core/src/main.rs`: `fn read_git_show_file( cwd: Option<&std::path::Path>, head: &str, path: &str, ) -> std::io::Result<String>`
- `rinkaku-core/src/main.rs`: `fn read_stdin_diff() -> anyhow::Result<String>`
- `rinkaku-core/src/render.rs`: `pub fn render(report: &Report, format: OutputFormat) -> Result<String, RenderError>`
- `rinkaku-core/src/main.rs`: `fn run_git_diff(base: &str, head: &str) -> anyhow::Result<String>`

```
fn garbage_input_note( diff_text: &str, report: &rinkaku_core::render::Report, ) -> Option<&'static str>
```

Depends on:
- `rinkaku-core/src/render.rs`: `pub struct Report { pub files: Vec<FileReport>, pub skipped: Vec<SkippedFile>, }`

... (6 more symbols, same shape)
````

The 128-line diff (test bodies included) becomes 59 lines of signatures
and their dependencies — the reviewer sees which functions changed and
what they touch, without reading every match arm by hand. On a larger
real diff — rinkaku's own [PR #7](https://github.com/hiro-o918/rinkaku/pull/7)
(a 2,254-line diff adding dependency resolution across 12 files) —
`rinkaku` produces 658 lines: about 29% of the original, while surfacing
cross-file dependencies that would otherwise require opening every
changed file to trace by hand.

`--format json` renders the same data as structured JSON instead
(`{"files": [{"path", "symbols": [{"name", "kind", "signature", "range",
"container", "dependencies", "omitted_matches"}]}], "skipped": [...]}`),
for piping into `jq` or another tool:

```sh
$ git show 1ccc183 --format="" | rinkaku --format json | jq '.files[0].symbols[0]'
```

```json
{
  "name": "main",
  "kind": "Function",
  "signature": "fn main() -> anyhow::Result<()>",
  "range": { "start": 73, "end": 118 },
  "container": null,
  "dependencies": [
    {
      "signature": "pub trait Resolver { fn resolve(&self, name: &str) -> Vec<ResolvedSymbol>; }",
      "path": "rinkaku-core/src/deps.rs"
    },
    {
      "signature": "pub fn analyze_diff( diff_text: &str, read_file: impl Fn(&str) -> std::io::Result<String>, resolver: Option<&dyn Resolver>, ) -> Result<Report, AnalyzeError>",
      "path": "rinkaku-core/src/pipeline.rs"
    },
    {
      "signature": "fn build_resolver( cli: &Cli, diff_text: &str, diff_read_file: impl Fn(&str) -> std::io::Result<String>, head: Option<&str>, cwd: Option<&std::path::Path>, ) -> anyhow::Result<Option<TagsResolver>>",
      "path": "rinkaku-core/src/main.rs"
    },
    {
      "signature": "fn read_git_show_file( cwd: Option<&std::path::Path>, head: &str, path: &str, ) -> std::io::Result<String>",
      "path": "rinkaku-core/src/main.rs"
    },
    {
      "signature": "fn read_stdin_diff() -> anyhow::Result<String>",
      "path": "rinkaku-core/src/main.rs"
    },
    {
      "signature": "pub fn render(report: &Report, format: OutputFormat) -> Result<String, RenderError>",
      "path": "rinkaku-core/src/render.rs"
    },
    {
      "signature": "fn run_git_diff(base: &str, head: &str) -> anyhow::Result<String>",
      "path": "rinkaku-core/src/main.rs"
    }
  ],
  "omitted_matches": 0
}
```

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
## crates/ty_server/src/server/lazy_work_done_progress.rs

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
the current `main`, the same diff produces 295 output lines (down from
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

## Development

Requires a Rust toolchain (pinned in [`rust-toolchain.toml`](rust-toolchain.toml);
`rustup` will install it automatically).

The workspace has two crates: `rinkaku-core` (the pure diff-condensation
library, published standalone so it can be embedded in other tools) and
`rinkaku` (the thin CLI binary, depending on `rinkaku-core`).

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

## License

MIT
