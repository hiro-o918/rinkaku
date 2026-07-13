# GitHub Action

The composite [`action.yaml`](../action.yaml) runs rinkaku against a
pull request's diff and posts (or updates) a **sticky PR comment**: a
[`--format mermaid`](cli.md#format-mermaid) call/dependency graph up
front — rendered natively by GitHub in the comment — with the full
Markdown outline collapsed underneath for anyone who wants
signature-level detail.

## Minimal workflow

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

## Inputs

| Input | Default | Purpose |
| --- | --- | --- |
| `version` | `latest` | Release tag to download |
| `binary` | *(none)* | Path to a pre-built rinkaku binary; skips download |
| `repo-path` | `.` | The checkout rinkaku should analyze |
| `base` | PR's base ref | Base ref to diff against |
| `github-token` | `${{ github.token }}` | Token used to post/update the comment |
| `comment` | `true` | Set `false` to skip posting; only emit outputs |

Outputs: `mermaid-path`, `markdown-path` (paths to the generated
report files, whether or not `comment: true`).

## Trusted-base posture

The snippet above is the simple case (a pinned release binary,
`permissions: pull-requests: write` scoped to only what posting a
comment needs). If you build rinkaku yourself instead of using a
release binary, **build it from the PR's base ref, not the PR head** —
a PR is exactly the input an attacker controls, and this job runs
with a write token before anyone has reviewed it.

The pattern this repository's own [dogfooding
workflow](../.github/workflows/rinkaku-report.yaml) uses:

1. Check out the PR's **base** ref at the job's default location, so
   `uses: ./` resolves *the action code itself* — not just the binary
   — from a trusted checkout.
2. Check out the PR head into a subdirectory purely as data, and pass
   it to this action via `repo-path`.

The same rationale applies to the [LLM-review recipe](llm-review.md):
always build the map from a trusted checkout, never from the branch
under review.

## Fork PRs

Fork PRs get a read-only token from the `pull_request` trigger
regardless of `permissions:`. The action detects this and falls back
to writing the report into the job's step summary instead of posting a
comment, so a fork PR's run still succeeds (exit 0) rather than
failing on a 403.
