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
(stdin/`--base` input, Markdown/JSON output), and 1-hop dependency
expansion (`--deps`, the tags-based `Resolver`) are implemented. Not
published to crates.io.

## Installation

TBD — not yet published. Once released, this section will cover
`cargo install rinkaku` and prebuilt binaries.

## Usage

```sh
# From a GitHub PR
gh pr diff 123 | rinkaku

# From a local git diff against a base branch
rinkaku --base main

# JSON output for feeding into another tool or LLM
rinkaku --base main --format json

# Skip dependency resolution (faster, no repo-wide index — see below)
rinkaku --base main --deps 0
```

### What it looks like

Running `rinkaku` on
[a real rinkaku commit](https://github.com/hiro-o918/rinkaku/commit/099fd83)
(a 73-line diff touching four test functions in `pipeline.rs`) produces:

```sh
$ git show 099fd83 --format="" | rinkaku
```

````markdown
## rinkaku-core/src/pipeline.rs

```
fn should_skip_deleted_file_without_reading_it()
```

Depends on:
- `rinkaku-core/src/render.rs`: `pub struct Report { pub files: Vec<FileReport>, pub skipped: Vec<SkippedFile>, }`
- `rinkaku-core/src/pipeline.rs`: `pub fn analyze_diff( diff_text: &str, read_file: impl Fn(&str) -> std::io::Result<String>, resolver: Option<&dyn Resolver>, ) -> Result<Report, AnalyzeError>`
- `rinkaku-core/src/pipeline.rs`: `fn fake_reader( files: HashMap<&'static str, &'static str>, ) -> impl Fn(&str) -> std::io::Result<String>`

```
fn should_skip_binary_file_without_reading_it()
```

Depends on:
- `rinkaku-core/src/render.rs`: `pub struct Report { pub files: Vec<FileReport>, pub skipped: Vec<SkippedFile>, }`
- `rinkaku-core/src/pipeline.rs`: `pub fn analyze_diff( diff_text: &str, read_file: impl Fn(&str) -> std::io::Result<String>, resolver: Option<&dyn Resolver>, ) -> Result<Report, AnalyzeError>`
- `rinkaku-core/src/pipeline.rs`: `fn fake_reader( files: HashMap<&'static str, &'static str>, ) -> impl Fn(&str) -> std::io::Result<String>`

... (2 more symbols, same shape)
````

The 73-line diff (test bodies included) becomes 37 lines of signatures and
their dependencies — the reviewer sees which functions changed and what
they touch, without reading every reassigned string literal in the test
bodies. On a larger real diff — rinkaku's own
[PR #7](https://github.com/hiro-o918/rinkaku/pull/7) (a 2,254-line diff
adding dependency resolution across 12 files) — `rinkaku` produces 658
lines: about 29% of the original, while surfacing cross-file dependencies
that would otherwise require opening every changed file to trace by hand.

`--format json` renders the same data as structured JSON instead
(`{"files": [{"path", "symbols": [{"name", "kind", "signature", "range",
"container", "dependencies"}]}], "skipped": [...]}`), for piping into `jq`
or another tool:

```sh
$ git show 099fd83 --format="" | rinkaku --format json | jq '.files[0].symbols[0]'
```

```json
{
  "name": "should_skip_deleted_file_without_reading_it",
  "kind": "Function",
  "signature": "fn should_skip_deleted_file_without_reading_it()",
  "range": { "start": 201, "end": 226 },
  "container": null,
  "dependencies": [
    {
      "signature": "pub struct Report { pub files: Vec<FileReport>, pub skipped: Vec<SkippedFile>, }",
      "path": "rinkaku-core/src/render.rs"
    },
    {
      "signature": "pub fn analyze_diff( diff_text: &str, read_file: impl Fn(&str) -> std::io::Result<String>, resolver: Option<&dyn Resolver>, ) -> Result<Report, AnalyzeError>",
      "path": "rinkaku-core/src/pipeline.rs"
    },
    {
      "signature": "fn fake_reader( files: HashMap<&'static str, &'static str>, ) -> impl Fn(&str) -> std::io::Result<String>",
      "path": "rinkaku-core/src/pipeline.rs"
    }
  ]
}
```

### `--deps`

`--deps 1` (the default) resolves each changed symbol's 1-hop dependencies
by indexing every file `git ls-files` tracks in the repository — this
makes the output more useful (you see what a changed function calls) but
costs an up-front repo-wide scan. `--deps 0` skips resolution entirely
(no "Depends on" sections, no repository scan), which is significantly
faster on large repositories: in QA, running against a large Rust
monorepo (astral-sh/ruff, tens of thousands of files) took ~9.5s with
`--deps 1` versus ~0.003s with `--deps 0` for the same diff. Prefer
`--deps 0` for quick iteration or CI checks where the dependency context
isn't needed.

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

### Known limitations

- **No type resolution (by design, ADR 0003)**: dependency resolution
  matches referenced names against definitions by name alone, with no
  type information — it cannot disambiguate overloads, shadowed names, or
  same-named symbols in unrelated modules. A future `Resolver`
  implementation backed by an LSP server (pyright, gopls, rust-analyzer,
  ...) is planned as a higher-precision, opt-in alternative for v2+; see
  the Roadmap below.
- **Same-name matches are ranked, not resolved**: when several
  definitions share a referenced name, they are ranked by path proximity
  to the referencing file (same file > same directory > shared path
  prefix depth > other) and only the top 3 are shown; the rest are
  reported as a count (`(+N more definitions matched by name)` in
  Markdown, `omitted_matches` in JSON) rather than silently dropped or
  listed in full. This bounds "Depends on" noise but does not guarantee
  the top 3 include the actually-referenced definition.
- **`_` and single-character identifiers are never resolved**: they are
  filtered out of referenced names entirely, since under name-only
  resolution they match too many unrelated definitions to be useful.
- **The `--deps 1` indexing prefilter has limited effect when a diff
  references common standard-library-style names**: `TagsResolver::new`
  skips parsing files whose content cannot contain any referenced name at
  all (measured ~88% fewer files parsed, ~8x faster indexing on a
  same-language-only reference set — see the PR description for the full
  numbers). But a name like `Vec`, `Option`, `String`, `Some`, or `Ok`
  appears in nearly every Rust file in a real codebase, so a diff whose
  referenced names include several of these sees little to no reduction
  (measured ~93% of files still parsed on one real-world diff). The
  prefilter is a substring match over raw file content, not scoped to
  actual definitions, so it cannot distinguish "defines `Vec`" from
  "mentions `Vec`" without also being safe against false negatives (see
  `deps.rs`'s `should_parse_file` doc comment) — narrowing this further is
  left for a future iteration. The dominant cost in `--base` mode remains
  the per-file `git show` subprocess invocation for reading tracked files
  (unrelated to this prefilter, and unaddressed — see `deps.rs`'s
  performance doc comment).

### Roadmap / not yet done

- LSP-backed `Resolver` implementations (pyright, gopls, rust-analyzer,
  ...) as a higher-precision, opt-in alternative to the v1 tags-based
  `Resolver`.
- Release automation (release-please, cross-compiled binary publishing)
  — intentionally deferred out of the bootstrap PR; tracked as a
  follow-up.

### Known limitations (v1 tags-based `Resolver`)

- **Name-only matching produces false-positive dependencies on common
  identifiers.** The v1 `Resolver` matches referenced names against a
  repo-wide index by name alone, with no scope or type awareness. On
  repositories with common short identifiers (Python's `_` placeholder
  convention, generic type names like `Data`/`Result`/`Client` reused
  across modules, standard-library names shadowed by an unrelated
  same-named local type), this can attach unrelated, noisy "Depends on"
  entries to a symbol — observed in QA against real OSS diffs (e.g. a
  builtin `pathlib.Path` reference resolving to an unrelated `class Path`
  in a test fixture; dozens of unrelated `def _(...)` matches inflating
  a single symbol's dependency list). This is the main motivation for the
  planned LSP-backed `Resolver` above, which would use real scope/type
  information instead of name matching.

## License

MIT
