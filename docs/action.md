# GitHub Action

This repository ships a
[composite action](../action.yaml) that runs rinkaku against a pull
request's diff and posts (or updates) a **sticky PR comment**. This
page is a setup guide for wiring it into a **different** repository's
workflow. For the output format itself, see
[CLI reference](cli.md).

## What it does

On each pull request, the action:

1. **Resolves a rinkaku binary** — downloads a GitHub Release asset by
   default, or uses a caller-provided binary (see [Inputs](#inputs)).
2. **Runs it against the PR's diff**, producing a Markdown report and
   (when the resolved binary supports them) a `--format mermaid` graph
   and a `--format digest` "API changes" summary.
3. **Composes the mermaid graph and the digest into a single comment
   body** — the mermaid graph up front (rendered natively by GitHub,
   with a one-line color legend underneath), an "API changes" digest
   collapsed in a `<details>` section below it (added/signature-changed/
   removed symbols only — ADR 0036) — and posts or updates a **sticky**
   comment on the PR: every run on the same PR edits the same comment
   (matched by an HTML marker) instead of piling up a new one per push.
   The full Markdown report is still generated and exposed via the
   `markdown-path` output, it just no longer inflates the comment body
   itself — see [Outputs](#outputs).

On a fork PR the run still succeeds (exit 0) — see
[Fork PR fallback](#fork-pr-fallback) below.

## Quick start

Add a workflow like this to the target repository, e.g.
`.github/workflows/rinkaku-report.yaml`:

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

`fetch-depth: 0` and the explicit `git fetch` of the base branch are
both needed so rinkaku's internal `--base "origin/<base-ref>"` diff
has the base commit available locally.

Pin `hiro-o918/rinkaku@main` to a release tag or commit SHA once
you've verified the action works for your repository — the same
"don't trust a moving ref" reasoning as any other third-party action.

## Getting a rinkaku binary

The action resolves the binary it runs in one of two ways, controlled
by the `binary` input:

- **Default — download a release**: leave `binary` unset. The action
  downloads the `version` release tag (default `latest`) from
  [GitHub Releases](https://github.com/hiro-o918/rinkaku/releases) for
  the runner's platform (`x86_64`/`aarch64`, Linux/macOS) and uses
  that. Right choice for the vast majority of external callers — the
  Quick start above uses it.
- **Caller-provided binary**: set `binary` to the path of an
  already-built rinkaku binary; the download is skipped entirely.
  This exists for callers building rinkaku themselves — most notably
  to preserve the [trust boundary](#trust-boundary) below — and is
  what this repository's own [dogfooding
  workflow](../.github/workflows/rinkaku-report.yaml) does
  (`cargo build --release -p rinkaku` from a trusted checkout, then
  `binary: ${{ github.workspace }}/target/release/rinkaku`).

## Inputs

| Input | Required | Default | Description |
| --- | --- | --- | --- |
| `version` | No | `"latest"` | Release tag to download, e.g. `"v0.4.1"`. Ignored when `binary` is set. |
| `binary` | No | `""` | Path to an already-built rinkaku binary to use instead of downloading a release asset. |
| `repo-path` | No | `"."` | Path to the git checkout rinkaku should analyze. Defaults to the current directory, which is only correct when the action's own steps run inside that checkout. |
| `base` | No | `${{ github.event.pull_request.base.ref }}` | Base ref to diff against, passed through to `rinkaku --base`. |
| `github-token` | No | `${{ github.token }}` | Token used to read/write the PR comment. Read-only on a fork PR regardless — see [below](#fork-pr-fallback). |
| `comment` | No | `"true"` | Whether to post/update a sticky PR comment. When `false`, only the report files exposed via outputs are produced (for callers composing their own comment). |

## Outputs

| Output | Description |
| --- | --- |
| `mermaid-path` | Path to the generated mermaid report file. Empty when the resolved rinkaku binary predates `--format mermaid` (see `markdown-only`). |
| `digest-path` | Path to the generated "API changes" digest file (ADR 0036). Empty when the resolved rinkaku binary predates `--format digest` (see `markdown-only`). |
| `markdown-path` | Path to the generated Markdown report file. Always produced, but no longer embedded in the sticky comment body — only the mermaid graph and digest are (see [What it does](#what-it-does)). |
| `markdown-only` | `"true"` when the resolved rinkaku binary predates `--format mermaid` and/or `--format digest`, and the comment fell back to a plain note in place of whichever section(s) are unavailable. |

## Trust boundary

A pull request is exactly the input an attacker controls, and this
job runs with `pull-requests: write` before anyone has reviewed the
PR. The **binary** that inspects the diff must not itself come from
the diff it is inspecting.

As an **external caller** using `uses: hiro-o918/rinkaku@<pinned>`,
you already get half of this for free: the action code itself comes
from the pinned ref of this repository, not from the calling PR.
What's left in your hands is the **binary**:

- The default (downloading a tagged release) is a reasonable choice
  for most repositories — the binary comes from this repository's own
  release pipeline, not from the PR under review.
- If you build rinkaku yourself instead (the `binary` input), build
  it from the PR's **base** ref, not the PR head, for the same
  reason: letting a PR supply the very tool that inspects it defeats
  the point of running that tool with a write token.

This repository's own [dogfooding
workflow](../.github/workflows/rinkaku-report.yaml) enforces a
stronger version of this rule than most external callers need,
because it also has to protect the **orchestration code**
(`action.yaml` and `compose_and_post_comment.sh` at `uses: ./`): it
checks out the PR's base ref at the job's default location (so
`uses: ./` resolves the action from that trusted checkout) and
checks out the PR head into a subdirectory purely as data, passed to
the action via `repo-path`.

The same reasoning applies to the
[LLM-review recipe](llm-review.md) — always build the map from a
trusted checkout, never from the branch under review.

## Fork PR fallback

A `pull_request`-triggered workflow against a fork PR receives a
read-only `GITHUB_TOKEN` from GitHub, no matter what the workflow's
`permissions:` block declares. The action detects this
(`github.event.pull_request.head.repo.fork`) and skips the
comment-post attempt entirely, writing the composed report to
`$GITHUB_STEP_SUMMARY` instead — the job still succeeds rather than
failing on a 403. The same fallback also covers any other unexpected
403 when posting (e.g. an org policy further restricting
`GITHUB_TOKEN`), not just the fork case.
