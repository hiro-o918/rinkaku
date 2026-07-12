# 0005. Auto-clone into a cache for PR URLs outside a local clone

- Status: accepted
- Date: 2026-07-12

## Context

ADR 0004's `--pr` mode requires running inside a local clone whose
`origin` is the PR's repository: `gh pr view` itself works anywhere when
given a URL, but the subsequent `git fetch`, `git show`-based file reads,
and the dependency resolver's `git ls-files` index all need a local git
repository. A PR URL, however, carries everything needed to identify the
repository — only a bare PR number is inherently ambiguous. ADR 0004
rejected a clone-less mode built on the GitHub HTTP API; that rejection
was about the API approach (HTTP client, token handling, per-file fetch
cost), not about the goal of working outside a clone.

## Decision

When `--pr` is given a URL and the current directory is not a clone of
that URL's repository (its `origin` does not match, or it is not a git
repository at all), rinkaku clones the repository as a blobless partial
clone (`--filter=blob:none`) into a per-repository cache directory
(`$RINKAKU_CACHE_DIR`, falling back to `$XDG_CACHE_HOME/rinkaku`, then
`~/.cache/rinkaku`) and runs the existing `--pr` pipeline with that
directory as the working repository. An existing cache entry is reused
and updated with `git fetch`. Cloning is delegated to `gh repo clone`
so authentication stays with `gh` (ADR 0004's auth stance). A bare PR
number keeps requiring a local clone, and a URL matching the current
clone keeps using it — behavior inside a matching clone is unchanged.

## Alternatives

- **GitHub HTTP API for diff and file contents**: rejected in ADR 0004;
  the reasons (new HTTP client, token handling, rate-limit-prohibitive
  per-file fetches for the dependency index) still hold. The git
  protocol via a partial clone avoids all three.
- **Discover an existing local clone (ghq or a `--repo-dir` flag)**:
  helps only users who already have a clone and couples rinkaku to a
  workspace-layout convention; the cache clone covers everyone,
  including CI. A found matching clone in the cwd is still preferred.
- **Full or shallow (`--depth`) clone into the cache**: a full clone
  pays the whole history up front for large repositories; a shallow
  clone breaks `git diff <base>...<head>` merge-base computation and
  fetch-deepening semantics. Blobless keeps the full DAG (correct
  merge-bases) while fetching blobs lazily only as `git show` needs
  them.

## Consequences

- `rinkaku --pr <URL>` works from any directory; the first run against a
  repository pays a tree-only clone, later runs pay a fetch.
- The dependency resolver (`--deps 1`) triggers lazy blob downloads in a
  cache clone — slower than a warm local clone; `--deps 0` stays cheap.
- rinkaku gains a managed cache directory; entries are plain git repos
  the user can delete freely. No eviction policy for now — revisit if
  cache growth becomes a real complaint.
- Private repositories need `gh` auth for the clone and `git` credentials
  for later fetches (`gh auth setup-git` covers both); documented in the
  README rather than handled specially.
- ADR 0004's "requires running inside a local clone" constraint is
  narrowed to bare-number arguments; its rejection of the HTTP API
  approach stands.
