# 0004. PR input mode via gh CLI within a local clone

- Status: accepted
- Date: 2026-07-12

## Context

rinkaku's primary use case is reviewing pull requests, but neither input
mode makes that a one-step operation. stdin mode (`gh pr diff N |
rinkaku`) reads changed files off the working tree, so it fails unless
run inside a checkout that already matches the PR head. `--base` mode
reads file content via `git show`, which works without checking out the
PR branch — but only after the user manually fetches the PR head and
figures out the right base ref. Reviewing a PR today means: cd into the
clone, `git fetch origin pull/N/head`, then
`rinkaku --base origin/<base> --head FETCH_HEAD`.

## Decision

Add a `--pr <url|number>` input mode that automates exactly that manual
sequence: resolve the PR's base branch and head via the `gh` CLI, fetch
the PR head ref with `git`, and run the existing `--base`/`--head`
pipeline against the fetched objects. The mode requires running inside a
local clone of the target repository. `gh` becomes a runtime dependency
of this mode only (following the existing pattern of shelling out to
`git`); authentication is delegated to `gh` entirely. `rinkaku-core`
is not touched — the change is confined to the composition root.

## Alternatives

- **Clone-less mode (fetch diff and file contents from the GitHub
  API)**: rejected for now. It would introduce an HTTP client and token
  handling, and dependency resolution (ADR 0003) indexes every tracked
  file in the repository — per-file API fetches are rate-limit
  prohibitive, and the tarball-download workaround adds temp-dir
  management plus a `git ls-files` replacement. Too much machinery for
  the benefit while a clone is cheap to have.
- **Keep stdin-only and document the manual fetch flow**: zero code, but
  leaves the tool's primary use case a multi-step procedure that users
  must rediscover each time.
- **Teach stdin mode to read from git objects instead of the working
  tree**: impossible in general — stdin diffs have unknown provenance,
  so there is no commit to resolve file contents against.

## Consequences

- Reviewing a PR becomes one command inside a clone, without checking
  out or dirtying the working tree.
- `--pr` is mutually exclusive with `--base` and with stdin input; the
  CLI surface grows by one mode.
- A local clone with a GitHub remote remains required. Clone-less
  operation stays out of scope until concrete demand justifies its
  complexity (revisit trigger for the rejected alternative above).
- `gh` must be installed and authenticated for `--pr`; the other modes
  keep working without it.
