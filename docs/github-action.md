# Using the GitHub Action from another repository

This repository ships a
[composite action definition](../action.yaml) that runs rinkaku against a
pull request's diff and posts (or updates) a sticky PR comment. This page
is a setup guide for using that action from a **different** repository.
For what the action's output looks like and the `--format mermaid`/
`--format md` flags it wraps, see
[CLI usage and output format](cli-usage.md) and
[ADR 0021: mermaid output format](adr/0021-mermaid-output-format.md).

## What it does

On each pull request, the action:

1. Resolves a rinkaku binary (downloads a GitHub Release asset by default,
   or uses a caller-provided binary — see [Inputs](#inputs)).
2. Runs it against the PR's diff, producing a Markdown report and (when the
   resolved binary supports it) a `--format mermaid` graph.
3. Composes both into a single comment body — the mermaid graph up front
   (rendered natively by GitHub), the full Markdown outline collapsed
   underneath — and posts or updates a **sticky** comment on the PR: every
   run on the same PR edits the same comment (matched by an HTML marker)
   instead of piling up a new one per push.

On a fork PR, the `pull_request` trigger's token is read-only regardless of
the workflow's `permissions:` block. The action detects this ahead of time
and, rather than failing, writes the report into the job's
`$GITHUB_STEP_SUMMARY` instead — the run still exits 0.

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

`fetch-depth: 0` and the explicit `git fetch` of the base branch are both
needed so rinkaku's internal `--base "origin/<base-ref>"` diff has the base
commit available locally.

Pin `hiro-o918/rinkaku@main` to a release tag or commit SHA once you've
verified the action works for your repository, rather than tracking `main`
indefinitely — the same "don't trust a moving ref" reasoning as any other
third-party action.

## Getting a rinkaku binary

The action resolves the binary it runs in one of two ways, controlled by
the `binary` input:

- **Default — download a release**: leave `binary` unset. The action
  downloads the `version` release tag (default `latest`) from
  [GitHub Releases](https://github.com/hiro-o918/rinkaku/releases) for the
  runner's platform (`x86_64`/`aarch64`, Linux/macOS) and uses that.
  This is the right choice for the vast majority of external callers — the
  Quick start example above uses it.
- **Caller-provided binary**: set `binary` to a path of an already-built
  rinkaku binary, and the download step is skipped entirely. This exists
  for callers building rinkaku themselves — most notably to preserve the
  trust boundary described below — and is what this repository's own
  [dogfooding workflow](../.github/workflows/rinkaku-report.yaml)
  does (`cargo build --release -p rinkaku` from a trusted checkout, then
  `binary: ${{ github.workspace }}/target/release/rinkaku`).

## Inputs

| Input | Required | Default | Description |
| --- | --- | --- | --- |
| `version` | No | `"latest"` | rinkaku release tag to download from GitHub Releases, e.g. `"v0.4.1"`. Ignored when `binary` is set. |
| `binary` | No | `""` | Path to an already-built rinkaku binary to use instead of downloading a release asset. |
| `repo-path` | No | `"."` | Path to the git checkout rinkaku should analyze (what `--base`'s `git diff`/`git show` resolve against). Defaults to the current directory, which is only correct when the action's own steps run inside that checkout. |
| `base` | No | `${{ github.event.pull_request.base.ref }}` | Base ref to diff against, passed through to `rinkaku --base`. |
| `github-token` | No | `${{ github.token }}` | Token used to read/write the PR comment via `gh`/`gh api`. Read-only on a fork PR regardless of this input or the workflow's `permissions:` block — see the `comment` input and the fork PR note below. |
| `comment` | No | `"true"` | Whether to post/update a sticky PR comment. When `false`, the action only produces the report files exposed via its outputs (`mermaid-path`/`markdown-path`), for callers that want to compose their own comment. Also downgrades to the step-summary fallback automatically when posting isn't possible at all (see the fork PR note below) — this happens regardless of `comment`'s value. |

### Outputs

| Output | Description |
| --- | --- |
| `mermaid-path` | Path to the generated mermaid report file. Empty when the resolved rinkaku binary predates `--format mermaid` (see `markdown-only`). |
| `markdown-path` | Path to the generated Markdown report file. |
| `markdown-only` | `"true"` when the resolved rinkaku binary predates `--format mermaid` and the run fell back to a Markdown-only report. |

## Security note: trust boundary

A pull request is exactly the input an attacker controls, and this job runs
with `pull-requests: write` before anyone has reviewed the PR. The
**binary** that inspects the diff must not itself come from the diff it is
inspecting.

This repository's own
[dogfooding workflow](../.github/workflows/rinkaku-report.yaml)
enforces a stronger version of this rule than most external callers need,
because it also has to protect the **orchestration code** (`action.yaml`
and `compose_and_post_comment.sh` at `uses: ./`): it checks out the PR's
base ref at the job's default location (so `uses: ./` resolves the action
from that trusted checkout) and checks out the PR head into a subdirectory
purely as data, passed to the action via `repo-path`.

As an **external caller** using `uses: hiro-o918/rinkaku@<pinned>`, you
already get the orchestration-code half of this for free: the action code
itself comes from the pinned ref of this repository, not from the calling
PR. What's left in your hands is the **binary**:

- The default (downloading a tagged release) is a reasonable choice for
  most repositories — the binary comes from this repository's own release
  pipeline, not from the PR under review.
- If you build rinkaku yourself instead (the `binary` input), build it from
  the PR's **base** ref, not the PR head, for the same reason: letting a
  PR supply the very tool that inspects it defeats the point of running
  that tool with a write token.

## Fork PR fallback

A `pull_request`-triggered workflow run against a fork PR receives a
read-only `GITHUB_TOKEN` from GitHub, no matter what the workflow's
`permissions:` block declares. The action detects this
(`github.event.pull_request.head.repo.fork`) and skips the comment-post
attempt entirely, writing the composed report to `$GITHUB_STEP_SUMMARY`
instead — the job still succeeds rather than failing on a 403. The same
fallback also covers any other unexpected 403 when posting (e.g. org
policy restricting `GITHUB_TOKEN` further), not just the fork case.
