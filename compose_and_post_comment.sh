#!/usr/bin/env bash
# Composes rinkaku's mermaid + API-changes-digest output into a single PR
# comment body and posts it as a sticky comment (identified by MARKER
# below), updating an existing one instead of piling up duplicates on
# every push. Falls back to $GITHUB_STEP_SUMMARY when posting isn't
# possible at all (a fork PR's `pull_request` token is read-only
# regardless of the workflow's `permissions:` block).
#
# The comment's <details> section holds the digest (ADR 0036: added/
# signature-changed/removed symbols only), not the full Markdown report —
# the full report stays available as this action's `markdown-path`
# output for callers that want it, it just no longer inflates the PR
# comment itself.
#
# Pulled out of action.yaml's inline `run:` block so it has a path a human
# (or this repository's own dogfooding workflow) can execute directly for
# dynamic verification, rather than only ever running inside a GitHub Actions
# composite step.
#
# Optional environment (both PATH inputs below are optional in the same
# sense MERMAID_PATH always was: empty/unset when the resolved rinkaku
# binary doesn't support the corresponding `--format`, action.yaml's
# bootstrap-safety fallback):
#   MERMAID_PATH   path to the generated mermaid report. Omitting it
#                  drops the mermaid section entirely instead of
#                  embedding an empty fence.
#   DIGEST_PATH    path to the generated API-changes digest (ADR 0036).
#                  Omitting it drops the <details> section's content
#                  down to a short "no digest available" note instead of
#                  an empty block.
#   REPO           "owner/repo", passed to `gh api`. Required unless
#                  DRY_RUN=1.
#   PR_NUMBER      pull request number to comment on. Required unless
#                  DRY_RUN=1.
#   GH_TOKEN       token `gh` uses for authentication. Required unless
#                  DRY_RUN=1.
#   IS_FORK_PR     "true" when the PR head repo is a fork (GitHub Actions'
#                  `github.event.pull_request.head.repo.fork`) — skips the
#                  comment-post attempt entirely and writes to
#                  $GITHUB_STEP_SUMMARY instead, since a fork PR's
#                  `pull_request`-event token is read-only regardless of
#                  the workflow's `permissions:` block and would just 403.
#   GITHUB_STEP_SUMMARY  path GitHub Actions exposes for step-summary
#                  markdown; written to on the fork/403 fallback path.
#                  When unset (e.g. local dynamic verification), the
#                  fallback path prints to stdout instead.
#   DRY_RUN        when set to "1", prints the composed body to stdout
#                   instead of calling `gh api`/writing the step summary —
#                   used for local dynamic verification without a real PR
#                   to post to.
set -euo pipefail

# Byte-, not character-, semantics for every `${#var}`/`${var:0:n}` use
# below: under the default UTF-8 locale, bash's own string-length and
# substring operators count *characters*, which can silently overshoot
# GitHub's byte-based comment-size cap and — worse — split a multi-byte
# character in half mid-sequence (rinkaku's own output contains multi-byte
# glyphs like the cycle-warning ⚠️). `LC_ALL=C` makes both operators
# byte-exact; `truncate_utf8_safe` below additionally guards the boundary
# itself by backing off up to 3 bytes (the longest possible UTF-8
# continuation run) until the result decodes cleanly.
export LC_ALL=C

MARKER="<!-- rinkaku-report -->"

# GitHub's PR/issue comment body limit.
MAX_BODY_LENGTH=65536

# A mermaid document this large wouldn't render as a useful diagram on
# GitHub anyway (ADR 0021's node budget already keeps a healthy graph well
# under this), so past this size the section is replaced with a short note
# rather than spending the whole comment budget on an unrenderable-in-
# practice diagram and leaving nothing for the digest details.
MAX_MERMAID_LENGTH=32768

# One line spelling out the mermaid `classDef` colors (ADR 0021/0035) in
# English, since GitHub's rendered mermaid diagram carries no legend of
# its own — composed here, not by `render_mermaid`/`render_digest`,
# because it's prose about this comment's presentation, not data derived
# from the `Report` (ADR 0036's Decision). No emoji/color swatches (this
# repository avoids decorative glyphs in rendered output on principle,
# per ADR 0028's terminal-rendering rationale) — plain text names of the
# `classDef`s themselves.
MERMAID_LEGEND="_Legend: green = added · orange = API changed · gray dashed = removed · red heavy border = fan-in_"

# Truncates `text` to at most `budget` bytes, then backs off up to 3 more
# bytes (bounded: the longest a single UTF-8 character's continuation-byte
# run can be) until the result is valid UTF-8 on its own — a byte-count
# truncation can otherwise land inside a multi-byte character and emit a
# malformed tail byte sequence. Never called with a negative budget (both
# call sites below clamp their budget to >= 0 first).
truncate_utf8_safe() {
  local text="$1"
  local budget="$2"
  local truncated="${text:0:budget}"
  local attempt=0
  while [ "${attempt}" -lt 4 ] && ! printf '%s' "${truncated}" | iconv -f UTF-8 -t UTF-8 >/dev/null 2>&1; do
    truncated="${truncated:0:$((${#truncated} - 1))}"
    attempt=$((attempt + 1))
  done
  printf '%s' "${truncated}"
}

# `MERMAID_PATH`/`DIGEST_PATH` are empty (not just unset) on the
# markdown-only fallback path (action.yaml's bootstrap-safety check) —
# `${VAR:-}` reads as empty either way, so both "input never set" and
# "set to empty by the caller" collapse to the same "omit this section"
# branch below.
#
# `mermaid_oversized` is tracked separately from "no mermaid content at
# all" so the oversized case renders its explanatory note as plain text
# instead of nesting it inside a ```mermaid fence — a fence around prose
# is misleading (it reads as "here is the mermaid source," when the whole
# point of the note is that there isn't one) and mermaid itself would
# just fail to parse the note as a diagram anyway.
mermaid_content=""
mermaid_oversized=0
if [ -n "${MERMAID_PATH:-}" ]; then
  mermaid_content=$(cat "${MERMAID_PATH}")
  if [ "${#mermaid_content}" -gt "${MAX_MERMAID_LENGTH}" ]; then
    mermaid_content="*(mermaid graph omitted: it would exceed ${MAX_MERMAID_LENGTH} bytes, past the point a diagram this size renders usefully on GitHub anyway)*"
    mermaid_oversized=1
  fi
fi

mermaid_section=""
if [ -n "${mermaid_content}" ] && [ "${mermaid_oversized}" -eq 1 ]; then
  mermaid_section="
${mermaid_content}

${MERMAID_LEGEND}
"
elif [ -n "${mermaid_content}" ]; then
  mermaid_section="
\`\`\`mermaid
${mermaid_content}
\`\`\`

${MERMAID_LEGEND}
"
fi

# `render_digest` (ADR 0036) already returns an empty string when there is
# no contract change to report — that's a legitimate "nothing to see
# here" for a PR with no added/changed/removed symbols, distinct from
# "no digest file at all" (the markdown-only bootstrap fallback), which
# gets its own explanatory note instead of a silently empty `<details>`.
digest_content=""
if [ -n "${DIGEST_PATH:-}" ]; then
  digest_content=$(cat "${DIGEST_PATH}")
fi
if [ -z "${digest_content}" ] && [ -n "${DIGEST_PATH:-}" ]; then
  digest_content="_(no API-surface changes detected in this diff)_"
elif [ -z "${digest_content}" ]; then
  digest_content="_(API-changes digest unavailable: resolved rinkaku binary predates \`--format digest\`)_"
fi

body_prefix="${MARKER}
## rinkaku PR report
${mermaid_section}
<details>
<summary>Details</summary>

"

body_suffix="
</details>
"

# Reserve space for everything except the digest section itself, then
# truncate that section to what's left — this is what keeps the mermaid
# graph (the human-first part of the comment) intact even under the size
# cap. A digest is bounded by the number of contract-changing symbols
# rather than total diff size (ADR 0036), so this is expected to fire far
# less often than it did for the old full-Markdown details, but the
# safety net stays for a pathologically large rename-everything PR.
truncation_note="

*(details truncated: too large for a single PR comment)*"
reserved=$((${#body_prefix} + ${#body_suffix} + ${#truncation_note}))
budget=$((MAX_BODY_LENGTH - reserved))

if [ "${budget}" -lt 0 ]; then
  budget=0
fi

if [ "${#digest_content}" -gt "${budget}" ]; then
  digest_content=$(truncate_utf8_safe "${digest_content}" "${budget}")
  digest_content="${digest_content}${truncation_note}"
fi

body="${body_prefix}${digest_content}${body_suffix}"

if [ "${DRY_RUN:-0}" = "1" ]; then
  printf '%s' "${body}"
  exit 0
fi

# Fork PRs (`github.event.pull_request.head.repo.fork == true`) get a
# read-only GITHUB_TOKEN from the `pull_request` trigger no matter what
# the workflow's own `permissions:` block declares — posting would just
# 403. Detecting this ahead of time (rather than only reacting to a 403
# after the fact) keeps the job green and gives a clear, deliberate
# fallback surface instead of a caught-error one.
if [ "${IS_FORK_PR:-false}" = "true" ]; then
  echo "::notice::PR head is a fork; skipping comment post (read-only token) and writing the report to the step summary instead"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '%s\n' "${body}" >>"${GITHUB_STEP_SUMMARY}"
  else
    printf '%s\n' "${body}"
  fi
  exit 0
fi

: "${REPO:?REPO is required}"
: "${PR_NUMBER:?PR_NUMBER is required}"

payload=$(jq -n --arg body "${body}" '{body: $body}')

# Finds an existing sticky comment by marker (first match; a PR should only
# ever accumulate one, since every run either updates it or creates the
# first one) and posts/updates it. A same-repo (non-fork) PR can still hit
# a 403 in less common setups (org policy restricting `GITHUB_TOKEN`
# further, branch protection quirks, etc.) — `IS_FORK_PR` above is the
# expected/deliberate case this function's caller falls back for; this
# function's own non-zero return covers every other `gh api` failure the
# same way, including the *lookup* call below (not just the final
# post/patch), since a read-only token 403s on that GET just as readily.
post_comment() {
  local existing_comment_id
  existing_comment_id=$(gh api "repos/${REPO}/issues/${PR_NUMBER}/comments" --paginate \
    --jq "[.[] | select(.body | startswith(\"${MARKER}\"))][0].id // empty") || return 1

  if [ -n "${existing_comment_id}" ]; then
    gh api "repos/${REPO}/issues/comments/${existing_comment_id}" \
      --method PATCH --input - <<<"${payload}" >/dev/null
  else
    gh api "repos/${REPO}/issues/${PR_NUMBER}/comments" \
      --method POST --input - <<<"${payload}" >/dev/null
  fi
}

if ! post_comment; then
  echo "::notice::posting the PR comment failed (likely a read-only token); writing the report to the step summary instead"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '%s\n' "${body}" >>"${GITHUB_STEP_SUMMARY}"
  else
    printf '%s\n' "${body}"
  fi
fi
