# 0007. Base `--pr` diffs on `baseRefOid` instead of the fetched branch tip

- Status: accepted
- Date: 2026-07-12

## Context

`--pr` mode (ADR 0004) resolves the PR's base branch name via `gh pr view`,
fetches that branch's current tip with `git fetch`, and runs
`git diff <fetched-base-tip>...<head>` through the same pipeline as
`--base` mode. This works for an open PR, whose base branch tip has not
advanced past what the PR was opened against in any way that matters for
a triple-dot diff. For a **merged** PR, however, the base branch tip has
by definition advanced to include the PR's own commits (the merge itself),
so the PR's head is now an ancestor of the base branch tip. A triple-dot
diff against an ancestor produces an empty diff — `git diff` succeeds and
prints nothing, `analyze_diff` returns an empty `Report`, `render` returns
an empty string, and rinkaku exits 0 having silently done nothing. There is
also no diagnostic today in this code path (see the companion fix that adds
one), which makes the silence particularly confusing: the command runs
successfully and produces no output for what looks like a normal review
request.

## Decision

Resolve `--pr` mode's diff base from the PR's `baseRefOid` — the base
branch's tip *at the time `gh pr view` reports it* — instead of fetching
the base branch by name and using whatever its tip happens to be right
now. `gh pr view --json ...,baseRefOid` already reports this alongside the
existing `baseRefName`/`headRefOid` fields, so this is an additive field on
the same API call, not a new one.

For an open PR, `baseRefOid` and the base branch's current tip are the
same commit (nothing has landed on top since the PR was opened), so this
is behavior-preserving for the common case. For a merged PR, `baseRefOid`
stays pinned to the commit the PR was actually diffed against at merge
time, so `git diff <baseRefOid>...<head>` reproduces the original PR diff
regardless of how far the base branch has moved since.

Getting the commit object for `baseRefOid` locally follows a small
availability cascade, since a commit reachable today from the base branch
tip may not yet be reachable from a stale local clone (a cache clone in
particular, ADR 0005/0006, may not have fetched recently):

1. Check whether the object already exists locally
   (`git cat-file -e <oid>^{commit}`) — the common case once the base
   branch has been fetched at all.
2. If not, fetch the base branch by name (as before) and re-check —
   `baseRefOid` is normally reachable from the base branch's history, so
   an ordinary branch fetch usually retrieves it.
3. If still not found (e.g. the base branch was force-pushed past that
   commit, or deleted after merge), fetch the oid directly
   (`git fetch origin <oid>`) — works against GitHub as long as the
   commit hasn't been actually garbage-collected server-side.
4. If that also fails, fall back to the fetched base branch tip (today's
   behavior) and print a `log::warn!` — better a possibly-wrong-for-merged-
   PRs diff with a warning than a hard failure, since this only degrades
   back to the pre-fix behavior rather than introducing a new failure mode.

## Alternatives

- **Diff against the merge commit's first parent**: only meaningful once
  a PR is known to be merged, which means branching the `--pr` code path
  on PR state and handling rebase-merges (where there is no single merge
  commit) as a separate case. Rejected for the extra branching and
  ambiguity; `baseRefOid` covers both open and merged PRs uniformly
  through the same field and the same diff command.
- **Use `gh pr diff` output directly instead of local `git diff`**: would
  need a separate parse path and loses the property ADR 0004 established —
  that the diff and the `git show`-based file reads (both for changed
  files and for the dependency index) are pinned to commits verified to
  exist in the local clone, not merely trusted from `gh`'s own diff
  rendering.

## Consequences

- `rinkaku --pr <url>` on a merged PR now reproduces the original PR diff
  instead of silently producing nothing.
- Open-PR behavior is unchanged: `baseRefOid` equals the base branch's
  current tip in the common case.
- `--pr` mode gains an availability cascade with a fetch-the-oid-directly
  step and a warn-and-fall-back last resort, both new failure/degradation
  modes worth knowing about when debugging an unexpected diff base.
- `PrInfo` grows a `base_ref_oid` field; `fetch_branch_head` (still used
  for the fallback step) is unchanged.
