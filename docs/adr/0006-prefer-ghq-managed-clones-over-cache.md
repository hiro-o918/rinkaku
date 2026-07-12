# 0006. Prefer existing ghq-managed clones over the cache clone

- Status: accepted
- Date: 2026-07-12

## Context

ADR 0005 makes `--pr <URL>` work from any directory by falling back to a
blobless clone in rinkaku's cache. Users who manage their repositories
with [ghq](https://github.com/x-motemen/ghq) usually already have a full
clone of the repository they are reviewing; cloning it again into the
cache duplicates disk usage and grows the cache with every new
repository. ADR 0005 rejected local-clone discovery as the *sole*
mechanism because it cannot help users who have no clone at all — that
reasoning does not apply to using discovery as a preference layer in
front of the cache fallback.

## Decision

When resolving the working repository for a PR URL, probe in order:

1. the current directory, when its `origin` matches the URL's
   owner/repo (unchanged from ADR 0004/0005);
2. an existing clone reported by `ghq list --full-path --exact
   <owner>/<repo>`, when `ghq` is on `PATH` — the first hit whose
   `origin` matches the URL's repository is used;
3. the cache blobless clone (ADR 0005).

ghq being absent, returning no hit, or returning only clones whose
`origin` does not match all fall through silently to the cache. rinkaku
never mutates a discovered clone beyond what `git fetch` does (updating
`FETCH_HEAD` and remote-tracking refs); the working tree is untouched.
This partially supersedes ADR 0005's rejection of clone discovery, which
now stands only against discovery *as the sole mechanism*.

## Alternatives

- **A generic search-path setting (e.g. `RINKAKU_REPO_ROOTS`)**: more
  configuration surface for a need ghq already expresses; can be added
  later without conflicting with this decision if non-ghq users ask.
- **Defaulting the cache location to the ghq root**: would plant
  rinkaku-owned blobless clones inside a directory layout ghq believes
  it manages, surprising both tools.
- **Requiring ghq (no cache fallback)**: rejected in ADR 0005 already;
  leaves users without a clone stranded.

## Consequences

- ghq users get zero cache growth for repositories they already have;
  first-run latency drops to a `git fetch` against their existing clone.
- Discovered clones see their remote-tracking refs advance as a side
  effect of the fetch — the same effect any manual `git fetch` has, but
  worth knowing when a user wonders why `origin/<branch>` moved.
- ghq becomes an opportunistic, optional runtime dependency: its absence
  changes nothing, so CI and non-ghq users are unaffected.
- The probe order (cwd, ghq, cache) is fixed and documented; if other
  clone managers need supporting, they slot in as additional probes
  rather than reshaping the flow.
