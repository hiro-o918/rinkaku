#!/usr/bin/env bash
# Composes rinkaku's mermaid + Markdown output into a single PR comment body
# and posts it as a sticky comment (identified by MARKER below), updating an
# existing one instead of piling up duplicates on every push.
#
# Pulled out of action.yaml's inline `run:` block so it has a path a human
# (or this repository's own dogfooding workflow) can execute directly for
# dynamic verification, rather than only ever running inside a GitHub Actions
# composite step.
#
# Required environment:
#   MERMAID_PATH   path to the generated mermaid report
#   MARKDOWN_PATH  path to the generated Markdown report
#   REPO           "owner/repo", passed to `gh api`/`gh pr comment`
#   PR_NUMBER      pull request number to comment on
#   GH_TOKEN       token `gh` uses for authentication
#
# Optional environment:
#   DRY_RUN        when set to "1", prints the composed body to stdout
#                   instead of calling `gh api` — used for local dynamic
#                   verification without a real PR to post to.
set -euo pipefail

MARKER="<!-- rinkaku-report -->"

# GitHub's PR/issue comment body limit. Truncating the collapsed Markdown
# section (rather than the mermaid graph, which must stay intact — a
# truncated mermaid document fails to render at all) keeps the comment
# postable even for a very large diff.
MAX_BODY_LENGTH=65536

: "${MERMAID_PATH:?MERMAID_PATH is required}"
: "${MARKDOWN_PATH:?MARKDOWN_PATH is required}"

mermaid_content=$(cat "${MERMAID_PATH}")
markdown_content=$(cat "${MARKDOWN_PATH}")

body_prefix="${MARKER}
## rinkaku PR report

\`\`\`mermaid
${mermaid_content}
\`\`\`

<details>
<summary>Details (full signature outline)</summary>

"

body_suffix="
</details>
"

# Reserve space for everything except the Markdown details section itself,
# then truncate that section to what's left — this is what keeps the
# mermaid graph (the human-first part of the comment) intact even under the
# size cap.
truncation_note="

*(details truncated: diff too large for a single PR comment)*"
reserved=$((${#body_prefix} + ${#body_suffix} + ${#truncation_note}))
budget=$((MAX_BODY_LENGTH - reserved))

if [ "${budget}" -lt 0 ]; then
  budget=0
fi

if [ "${#markdown_content}" -gt "${budget}" ]; then
  markdown_content="${markdown_content:0:budget}${truncation_note}"
fi

body="${body_prefix}${markdown_content}${body_suffix}"

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf '%s' "${body}"
  exit 0
fi

: "${REPO:?REPO is required}"
: "${PR_NUMBER:?PR_NUMBER is required}"

# Find an existing sticky comment by marker (first match; a PR should only
# ever accumulate one, since every run either updates it or creates the
# first one) so repeated pushes update in place instead of spamming the PR
# with a new comment each time.
existing_comment_id=$(gh api "repos/${REPO}/issues/${PR_NUMBER}/comments" --paginate \
  --jq "[.[] | select(.body | startswith(\"${MARKER}\"))][0].id // empty")

payload=$(jq -n --arg body "${body}" '{body: $body}')

if [ -n "${existing_comment_id}" ]; then
  gh api "repos/${REPO}/issues/comments/${existing_comment_id}" \
    --method PATCH --input - <<<"${payload}" >/dev/null
else
  gh api "repos/${REPO}/issues/${PR_NUMBER}/comments" \
    --method POST --input - <<<"${payload}" >/dev/null
fi
