# Architecture, known limitations, and roadmap

## Architecture

Lightweight ports & adapters: core extraction logic in `rinkaku-core` is
pure (no IO, no clock, no env), with tree-sitter parsing and future
LSP/process boundaries isolated behind traits (`LanguageSupport`,
`Resolver`) defined on the consumer side. See the
[contributor conventions](../CLAUDE.md) and
[Architecture Decision Records](adr) for details.

The workspace has three crates: `rinkaku-core` (the pure diff-condensation
library, published standalone so it can be embedded in other tools),
`rinkaku-tui` (the interactive terminal UI's view-models and `ratatui`
rendering, depending on `rinkaku-core`; see
[ADR 0016](adr/0016-tui-crate-and-stack.md)), and `rinkaku` (the thin
CLI binary, depending on both).

## Known limitations

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
  dropped or listed in full — see
  [When same-name matches are capped](cli-usage.md#when-same-name-matches-are-capped)
  for a real example. This bounds "Depends on" noise but does not
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
  the ruff `6237ecb4d` diff used in
  [CLI usage and output format](cli-usage.md), `--deps 1` dropped from
  ~9.5s pre-#9 to ~6.5s post-#9 — better, not solved). The prefilter is a
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
are exhausted — see the `Inner` example in
[CLI usage and output format](cli-usage.md#when-same-name-matches-are-capped)).
A future `Resolver` implementation backed by an LSP server (pyright,
gopls, rust-analyzer, ...) is planned as a higher-precision, opt-in
alternative for v2+; see [Roadmap](#roadmap-not-yet-done) below.

[`MAX_MATCHES_PER_NAME`]: ../rinkaku-core/src/deps.rs

## Roadmap / not yet done

- LSP-backed `Resolver` implementations (pyright, gopls, rust-analyzer,
  ...) as a higher-precision, opt-in alternative to the v1 tags-based
  `Resolver`.

## Release process

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
