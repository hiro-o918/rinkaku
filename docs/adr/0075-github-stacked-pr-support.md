# 0075. GitHub stacked PR support: review a stack in order with pre-computed analyses

- Status: proposed
- Date: 2026-08-30

## Context

`--pr` (ADR 0004) resolves exactly one pull request: `gh pr view` gives
its base/head commits, `git fetch` brings the head in, and the `--base`
pipeline runs once. Everything downstream — `TuiSession::run`,
`run_app`, `ReviewPorts::pr_context` (ADR 0048), the `w` key (ADR 0050)
— assumes a single `Report` and a single `PrContext` for the whole
session.

GitHub now ships stacked pull requests natively: a stack is an ordered
chain of PRs where each PR's base branch is the head branch of the PR
below it, and the GraphQL API exposes it as `PullRequest.stack`
(`PullRequestStack { number, baseRefName, size, entries { position,
pullRequest } }`). A reviewer working through a stack reads the PRs
bottom-up, one layer at a time, and the layers are small by design —
which is exactly the shape rinkaku's signature map is best at.

Reviewing a stack with today's `--pr` means one `rinkaku --pr N`
invocation per layer, each paying the full resolve-fetch-analyze cost
up front, and each starting a fresh annotation session: notes taken on
layer 1 are gone by the time layer 3 is open, and there is no way to
tell which PR a clipboard packet's annotations were about. Three
concrete gaps drive this ADR:

1. **Discovery**: rinkaku does not know a PR is part of a stack.
2. **Cost**: the reviewer waits for the analysis of each layer at the
   moment they want to read it, although the next layer is perfectly
   predictable while they read the current one.
3. **Attribution**: annotations (ADR 0048) carry a file/symbol location
   but no PR identity, so a session spanning several PRs cannot export
   them to the right place, and the status line cannot even say which
   PR is on screen.

## Decision

### D1. Discovery: `--pr` detects the stack; `--no-stack` opts out

When `--pr` is given, `main.rs` queries GitHub GraphQL once:

```graphql
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      stack {
        number
        baseRefName
        size
        entries(first: 50) {
          nodes {
            position
            pullRequest {
              number title headRefName baseRefName
              baseRefOid headRefOid state merged isDraft
            }
          }
        }
      }
    }
  }
}
```

via `gh api graphql -f query=... -F owner=... -F name=... -F number=...`
— the same `gh` dependency `fetch_pr_info` already requires. `stack ==
null` means the PR is not stacked and the existing single-PR path runs
unchanged. Otherwise, entries whose `pullRequest.merged` is `true` or
whose `state` is not `OPEN` are dropped (a merged layer's diff is
already in the trunk, so there is nothing left to review) and the
remaining entries, sorted by `position`, form the stack bottom-first.
The PR named on the command line becomes the initial cursor.

No new flag is needed to *enable* this: a PR either is in a stack or it
is not. `--no-stack` (a bool, only meaningful with `--pr`) is the
opt-out, for a reviewer who wants one layer in isolation.

Stack mode is **TUI-only**. `--format md|json|mermaid|digest` keeps
analysing just the requested PR: the machine-readable outputs are one
document per PR, and a multi-PR document format is a separate decision
with its own consumers (the GitHub Action, LLM prompts) to consult.

### D2. Every layer's SHAs are resolved before the TUI opens

Before any analysis runs, the driver fetches every stack PR's head in
one multi-refspec `git fetch` into named refs
(`refs/rinkaku/pull/<N>/head`) and reads each SHA back with
`git rev-parse` on that ref (`fetch_pr_heads`). The single-PR path's
`FETCH_HEAD` round trip is not reused: `FETCH_HEAD` is per-clone state,
so a second rinkaku process fetching in the same clone (a reviewer and
an agent, or two stack sessions) overwrites it between one process's
fetch and its `rev-parse` — the dogfooding run for this ADR hit exactly
that and analysed one layer against another's head. Each fetched
head is checked against the `headRefOid` the
GraphQL query reported (`ensure_fetched_head_matches`, unchanged), and
each PR's base is resolved with the existing `resolve_pr_base_sha`
cascade (ADR 0007) from that PR's own `baseRefOid`. For a stack PR,
GitHub's `baseRefOid` is the head of the branch below it, so a single
code path serves both the bottom layer (base = trunk) and every layer
above it.

Doing all fetches up front — rather than lazily per layer — is what
makes the background analysis in D3 safe: once the TUI is open, no
thread runs a git command that writes to the repository.

### D3. One background thread pre-computes the other layers into an in-memory cache

Only the initial PR (the cursor) is analysed synchronously, behind the
existing splash screen (ADR 0033). The remaining layers are analysed by
**one** background thread that, before each layer, picks the pending
layer nearest to the *current* cursor — the one just above first, then
the one just below, then outward. The driver publishes every cursor move
to the cache, so after `gt`/`gT` the next likely target is always the
one being analysed; a fixed bottom-up order would leave a reviewer who
walks down the stack waiting on every step. Waiting on the layer one is
reading is unavoidable; waiting on the next one is the cost this design
removes.

Results land in a `PrAnalysisCache` owned by `rinkaku-tui`: a
`Mutex<Vec<Slot>>` plus a `Condvar` and the published cursor, with
`Slot = Pending | InProgress | Ready(Arc<PrAnalysis>) | Failed(String)`
and `PrAnalysis { report, diff_text, pr: PrContext }`. The worker claims
a layer (`Pending` → `InProgress`) under the lock, so the choice and the
reservation are one step. The cache is data plus synchronisation and
nothing else — it has no knowledge of git or `gh`; the `rinkaku` binary
fills it, the TUI reads it.

Switching to a `Pending` or `InProgress` layer redraws the splash screen with
"Analyzing PR #N (k/n)…" and blocks on the condvar until the slot
resolves. A `Failed` layer reports its error in the status line and the
cursor stays where it was.

The cache is **in-memory only**; nothing is written to disk. An on-disk
cache keyed by head SHA would let a second `rinkaku --pr` invocation on
the same stack skip analysis entirely, but `Report` is `Serialize`-only
today, and a persisted cache adds an invalidation contract (analysis
options, rinkaku version, generated-path attributes) this ADR does not
want to define yet. Recorded as a follow-up, not a decision.

### D4. The event loop stays single-report; a driver loop re-enters it per layer

`run_app` keeps its one-`Report` shape. A new driver,
`TuiSession::run_stack`, owns the `StackPosition` (the ordered
`StackEntry { number, title, head_ref_name }` list plus a cursor) and
re-enters `run_app` once per visited layer with that layer's
`Arc<PrAnalysis>`, a per-layer `SourceReader` built by the binary from
the layer's head SHA (`--pr` mode's `git show`-backed reader, ADR
0047), and a per-layer `PrContext` on `ReviewPorts`.

`run_app` returns `AppExit { Quit, UpdateRequested, SwitchPr(usize) }`
instead of the previous `bool`. `SwitchPr` carries the target index;
the driver resolves it through the cache and loops. Annotation state
(`ReviewState`) is handed *into* `run_app` and handed back out on every
exit, so it survives layer switches; every other piece of `App` state
(tree cursor, jumplist, pane modes) is rebuilt per layer, since it is
derived from the `Report` being replaced.

`StackPosition` is pure data with pure transitions (`move_up`/`move_down`
clamp at the ends, `label()` renders `PR #43 2/3`) so its behaviour is
unit-testable without a terminal.

### D5. Keys and status line

`gt` moves to the next layer (up the stack, away from trunk) and `gT`
to the previous one (down, toward trunk) — vim's next/previous *tab*
pair, reusing the `g` prefix that already carries `gg`/`gd`/`gr`. A
stack of PRs reads naturally as a row of tabs, and the TUI's vocabulary
is vim's throughout. A `p`/`P` pair was considered (it would mirror
`n`/`N`) but `p` reads as *paste* to a vim user and adds another
case-paired binding for no mnemonic gain. Outside stack mode both keys
are no-ops. Both are listed in the `?` help overlay's Entry-screen group
in both locales.

The status line gains a leading `PR #43 2/3  |  ` segment in stack mode
only. The existing Tree-focus hint line is 74 columns; the 15-column
prefix pushes it past the 80-column budget (#196), so in stack mode the
hint segment drops its `enter: open` entry (the least discoverable loss:
Enter on a tree row is the universal "open" gesture, and the `?` overlay
still lists it). Outside stack mode the line is byte-for-byte what it
is today.

### D6. Annotations carry their PR; export groups by it

`Annotation` gains `pr_number: Option<u64>`. `ReviewState` gains
`current_pr: Option<u64>`, set by the driver on every layer switch and
stamped onto each annotation in `confirm_compose`. Non-stack sessions
leave both `None` and behave exactly as before.

The annotations list overlay prefixes each row with `#43` when the
annotation has a PR number.

**Sink A (GitHub PR review).** Annotations are grouped by `pr_number`.
The layer under the cursor at export time receives the chosen verdict
(approve / request changes / comment); every *other* layer with
annotations receives a `COMMENT` review carrying only its own comments.
A verdict is a statement about one PR, and the reviewer picked it while
looking at one PR; silently approving three PRs with one keypress is the
wrong default. `ReviewSubmitter::submit_review` keeps its signature but
is now called once per PR with that PR's own `PrContext` — the head SHA
differs per layer and GitHub validates comment anchors against it.

**Sink B (agent packet / clipboard).** Sections are grouped under `##
PR #43 — <title>` headings in stack order, so an agent reading the
packet knows which branch each note applies to. Annotations only carry
the number, so the renderer receives the stack's entries (number →
title) alongside them; a number without an entry renders as `## PR
#43`. A packet with no PR-numbered annotation renders exactly as today.

## Alternatives

- **`gh stack view --json`.** Requires the stack to be tracked locally
  by the `gh-stack` extension (`gh stack checkout` or `init`); a reviewer
  who only has the PR URL has no local tracking state. The GraphQL
  `stack` field is available to anyone who can read the PR. Rejected.
- **Walk `baseRefName` chains via `gh pr view`.** Would also cover
  stacks built by tools that chain PR bases without GitHub's native
  stack object (Graphite, ghstack, hand-made). Rejected for v1 because
  it cannot distinguish "PR whose base happens to be another PR's branch"
  from a deliberate stack, and because it needs one `gh` call per layer
  before the first screen. Recorded as a follow-up for non-native stacks.
- **Analyse every layer before opening the TUI.** Simplest possible
  model — no cache, no thread. Rejected: it multiplies the time to the
  first screen by the stack depth, which is precisely the cost this ADR
  exists to remove.
- **Deliver background results over an `mpsc` channel.** The event loop
  already polls one channel (ADR 0054's update check). Rejected: the
  results must outlive any single `run_app` invocation and be
  addressable by index when the reviewer jumps around, which means the
  receiver's messages would have to be stashed into exactly the shared
  structure the `Mutex<Vec<Slot>>` already is — with the borrow of the
  current `Report` fighting the push of the next one.
- **Make `run_app` multi-report.** Rejected: nearly every `App` field is
  derived from one `Report` (tree, jumplist, diff shape, blast radius
  cache), so a multi-report `App` would be a `Vec<App>` with a cursor in
  all but name. Re-entering the loop per layer is the same thing with
  none of the new invariants.
- **Verdict applies to every PR with annotations.** Fewer round trips,
  but it turns a per-PR decision into a batch one. Rejected (D6).
- **`p`/`P`, `}`/`{`, or `ctrl-n`/`ctrl-p` instead of `gt`/`gT`.** All
  free. `p`/`P` mirrors `n`/`N` but reads as *paste*; `}`/`{` would sit
  one unit above the `]`/`[` hunk pair but says nothing about PRs;
  `ctrl-n`/`ctrl-p` parallels `ctrl-o`/`ctrl-i` yet is emacs' idiom, not
  vim's. `gt`/`gT` is the only pair a vim user already knows as
  next/previous *tab*.
- **A fixed prefetch order chosen at start-up.** Simpler (a `Vec<usize>`
  computed once) but wrong the moment the reviewer moves against it:
  walking down the stack would wait on every layer. Rejected (D3).

## Consequences

- One additional `gh api graphql` call per `--pr` run, before the first
  splash frame. Non-stacked PRs pay only that call.
- A background thread runs `git diff`/`git show`/`git ls-files` reads
  concurrently with the TUI's own `git show` source reads. All are
  read-only, and every `git fetch` completes before the thread starts
  (D2), so no two processes write the repository at once.
- `TuiSession::run` (single PR) is unchanged in behaviour; internally it
  becomes the one-layer case of the driver, and `run_app`'s return type
  changes from `bool` to `AppExit`.
- `ReviewSubmitter::submit_review` is called once per PR in stack mode.
  `GhReviewSubmitter` needs no change — it already takes the `PrContext`
  per call.
- `Annotation` gains a field; every literal constructor in tests gains
  `pr_number: None`.
- `--format` outputs are untouched. The GitHub Action and every
  Markdown/JSON consumer see no difference.
- Quitting the TUI while the background thread is mid-analysis waits
  for the current layer's analysis to finish (a scoped thread joins on
  exit; a cancellation flag is checked between layers, not inside
  `analyze_diff`). The wait is bounded by one layer's analysis time.

## Amendment history of amended ADRs

- **Amends ADR 0048**: `Annotation` carries an optional PR number, and
  sink A's export posts one review per PR rather than one review per
  session. The verdict/menu flow, the `SelectionSnapshot` boundary, and
  the port definitions are unaffected.
- **Amends ADR 0020**: the status line's fixed hint segment shortens by
  one entry (`enter: open`) in stack mode only, to keep the 80-column
  budget with the new `PR #N k/n` prefix.
- **Amends ADR 0004**: `--pr` may resolve a chain of PRs, not one; the
  resolve-then-fetch design is unchanged and applied per layer.
