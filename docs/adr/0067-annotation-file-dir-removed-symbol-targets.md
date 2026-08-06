# 0067. Extend annotation targets to File, Dir, and removed-symbol rows

- Status: accepted
- Date: 2026-08-06
- Amends: [ADR 0048](0048-tui-review-actions.md) (review annotations),
  narrowing "v1 only supports symbol-anchored annotations" to the wider
  target set below; [ADR 0058](0058-tui-annotation-key-rebind.md) (the
  `a`/`A` key rebind and "annotation" vocabulary) is unaffected.

## Context

ADR 0048 shipped review annotations scoped to present symbol rows only
(`review_flow::derive_selection_snapshot`'s doc comment: "`None` ... on
any row that is not a present symbol"). In practice a reviewer's first
observation about a change is often not about one symbol: "this whole
file is dead code now", "this directory's structure doesn't make sense
anymore", or "this removed function should have stayed" (a removed-symbol
row, which the v1 gate excludes because it "has no graph presence to
jump from" the same way `selected_symbol_id` excludes it from
navigation — a different concern that the annotation gate copied
without re-examining). None of these have anywhere to go today; pressing
`a` on a File, Dir, or removed-symbol row is a silent no-op.

This ADR widens the annotation target set to File rows, Dir rows, and
removed-symbol rows, while keeping Source-screen compose out of scope
(ADR 0048's own scope boundary, untouched here) and keeping the same
destination-neutral `Annotation` primitive ADR 0048 established.

### GitHub API constraints, verified against current docs

Before choosing an export design for the new target kinds, this ADR
checked GitHub's REST API docs for both PR-comment-producing endpoints
(<https://docs.github.com/en/rest/pulls/reviews>,
<https://docs.github.com/en/rest/pulls/comments>, both
`apiVersion=2022-11-28`) rather than assuming symmetry between them:

- **`POST /repos/{owner}/{repo}/pulls/{pull_number}/comments`** (the
  standalone, single-comment endpoint) accepts `subject_type: "line"` or
  `"file"` on the request body; `line` is documented as "Required unless
  using `subject_type:file`". This endpoint supports true file-level PR
  comments.
- **`POST /repos/{owner}/{repo}/pulls/{pull_number}/reviews`** (the
  batch pending-review endpoint sink A already uses — open a review,
  attach every comment, submit with one verdict) documents its
  `comments[]` array items as `path`, `position`, `body`, `line`, `side`,
  `start_line`, `start_side` only. **`subject_type` is not a documented
  field of this endpoint's `comments[]` schema.** There is no
  batch-compatible way to attach a file-level (no-line) comment to a
  pending review alongside the existing inline comments.

These two endpoints are not interchangeable for this feature:
switching sink A to the standalone-comment endpoint per file-level note
would mean posting those comments as individual, immediately-visible PR
comments outside the pending-review batch — a different notification
and discard/undo behavior than every symbol-anchored annotation gets
today (ADR 0048's Alternatives already rejected per-comment posting for
exactly this reason: "creates a separate notification per call and
cannot be submitted/discarded as a unit"). Splitting one export into two
different API calls with two different UX guarantees, just to get a
`subject_type: "file"` comment into the same batch, is not a change this
ADR is willing to make for three target kinds whose common trait is
"has no diff line to point at anyway" (see Decision).

## Decision

**1. Target kind becomes explicit data, not inferred.** `AnnotationLocation`
(and `SelectionSnapshot`, `review_flow::derive_selection_snapshot`'s
output type) gains an `AnnotationTarget` enum:

```rust
pub enum AnnotationTarget {
    Symbol,
    RemovedSymbol,
    File,
    Dir,
}
```

Every construction site (the `From<SelectionSnapshot> for
AnnotationLocation` conversion, `derive_selection_snapshot`'s `Some(...)`
arms) sets this explicitly. No code infers a target's kind from
`symbol_id.is_some()`, a trailing slash on `path`, or any other sentinel
— the ADR 0048-era `AnnotationLocation` already leaves this ambiguous
(a `None` `symbol_id` today could mean either "not a symbol" or, after
this ADR, "a Dir/File row"), and sentinel-based inference is exactly the
kind of implicit-fallback shape ADR 0048's own Alternatives rejected for
export-sink selection. An explicit tag is one field, checked once at
render/export time, instead of every consumer re-deriving the same
classification from path shape.

**2. `derive_selection_snapshot` widens its row-kind gate.** Today it
returns `None` for `NodeKind::Dir`/`File`/`Section`/`TestGroup` and for a
removed `NodeKind::Symbol`. After this change:

- `NodeKind::File` → `AnnotationTarget::File`, `path` only, no
  `symbol_id`/`symbol_name`/`range`/`anchor`/`signature`.
- `NodeKind::Dir` → `AnnotationTarget::Dir`, `path` only, same empty
  optional fields.
- `NodeKind::Symbol` where `symbol_ref.removed` → `AnnotationTarget::RemovedSymbol`,
  `path` + `symbol_id` (the synthesized `{path}::{name}` id, `SymbolRef::id`'s
  own doc comment) + `symbol_name`, but no `range`/`anchor` — a removed
  symbol has no new-side line range to have one.
- `NodeKind::Section`/`TestGroup` stay `None` (see Decision 4).
- The existing present-symbol path is unchanged.

**3. Export partitions by anchor presence, not by target kind directly.**
A pure `partition_for_export` function (in `render.rs`, alongside the
existing sink renderers) splits `&[Annotation]` into two groups:

- **Anchored** (`anchor.or(range)` resolves to `Some`) → rendered by the
  existing `render_review_comments` path, posted as inline pending-review
  comments exactly as today. In practice this is every `Symbol`
  annotation whose range intersects a hunk — target kind is not consulted
  here, only whether an anchor resolved, so a future target kind that
  *does* carry a line (none of File/Dir/RemovedSymbol do) would fall into
  this bucket automatically.
- **Unanchored** (`File`, `Dir`, `RemovedSymbol`, and any `Symbol`
  annotation whose range never intersected a hunk — the same
  no-anchor case `render_review_comments`' `(1, 1)` fallback used to
  paper over) → collected into an **"Additional notes" section appended
  to the review body**, one bullet per annotation: `` `{path}`: {body
  first line} `` for a File/Dir target, `` `{path}` {symbol_name}
  (removed): {body first line} `` for a removed symbol, and the same
  format `render_agent_packet`'s heading already uses for an unanchored
  Symbol. Each bullet links no line (there is none to link), matching
  what a human reviewer would type by hand into the review body's free
  text today for exactly this kind of file/directory-scoped remark.

  This replaces the `(1, 1)` fallback: `render_review_comments` no longer
  invents a synthetic anchor for an annotation with no real one.
  `perform_export`'s `REVIEW_SUMMARY` + this section are composed into
  the review body posted with the batch, so the whole thing — inline
  comments and body-collected notes alike — remains one pending review
  submitted/discarded as a unit, preserving ADR 0048's Alternatives
  rejection of per-comment posting.

**4. `Section`/`TestGroup` stay out of scope.** Both are synthetic
grouping rows with no real file-tree path (`NodeKind::Section`/
`TestGroup`'s own doc comments; `crate::app`'s existing blast-radius
selection already special-cases them out as `NotApplicable` for the same
reason). Falling back to "annotate the nearest real file" would silently
attach a note to a location the reviewer never pointed at; there is no
natural real-path fallback for a `Section`, and `TestGroup`'s own file is
already directly annotatable as a `File` row one level up. `a` on either
row kind stays a silent no-op, unchanged from today.

**5. Agent-packet rendering (sink B) varies its heading by target kind.**
`render_agent_packet`'s per-annotation heading keeps its current
`{path}:{range} {symbol_name}` form for `Symbol`/`RemovedSymbol` targets
(a `RemovedSymbol` naturally renders as `{path} {symbol_name}` today,
since it carries no range) and adds:

- `File` → `## {path}`
- `Dir` → `## {path}/` (trailing slash distinguishes a directory heading
  from a file heading sharing the same path text, and mirrors how a
  shell/file-tree UI already marks directories)

**6. Existing-pane markers extend to File-row badges, Dir-row badges are
out of scope.** `AnnotationMarkers::file_counts` (keyed by path) already
counts every annotation under a path regardless of whether it carries a
`symbol_id` — `build_annotation_markers`'s own doc comment already
anticipated this ("a future file-level annotation is covered by
construction rather than needing a second field"), so a File-target
annotation increments the same counter a Symbol-target annotation
already does, and `row_view::entry_row_line`'s existing
`push_annotation_badge_span` call on the `File` arm needs no change to
pick it up — confirmed by a new test.

Dir-row badges are left out: `AnnotationMarkers` has no per-directory
counter today, and `Badges`' aggregation model (bottom-up, baked once at
tree-build time — `tree/mod.rs`'s own doc comment) is a poor fit for a
value that changes during the session; building a parallel
change-gated bottom-up aggregation for directories, mirroring
`Badges::merge`, is a disproportionate restructure for this PR's scope.
Left as explicit future work (see Consequences); a Dir-target annotation
is still fully captured, exported, and visible in the annotations-list
overlay — it just carries no tree-row badge yet.

**7. Compose overlay title and annotations-list entries degrade per
target kind**, reusing the same `(range, symbol_name)` degrade shape
`ui/review_overlay.rs`'s `compose_title_location`/
`annotations_list_entry_text` already implement for the no-range case —
a File/Dir/RemovedSymbol snapshot or annotation is simply another point
on that existing fallback ladder (no `range` → falls through to
`path`/`path + symbol_name`), not a new code path.

## Alternatives

- **Use the standalone `POST .../pulls/comments` endpoint (with
  `subject_type: "file"`) for File-target annotations, keeping the
  pending-review batch endpoint only for anchored ones.** Rejected: this
  endpoint posts immediately and individually — outside the pending
  review, with its own notification and no batch-submit/discard — the
  exact behavior ADR 0048's Alternatives rejected for *every* annotation
  in v1. Carving out one target kind to behave differently at export time
  (some of a session's notes are revocable by discarding the pending
  review, some are already public and can't be un-sent) is a surprising,
  inconsistent reviewer experience for a target-kind distinction the
  reviewer never asked to reason about.
- **Fall back Dir/removed-symbol annotations to the nearest anchorable
  line (e.g. line 1 of the first file under a Dir, or the removed
  symbol's last known line before deletion) and keep them as inline
  comments.** Rejected: a removed symbol's pre-deletion line number is
  base-side, not new-side — GitHub's review API comments are anchored to
  the *new* side of the diff (`side: "RIGHT"`, `review.rs`'s own request
  builder), so there is no new-side line to point at for a symbol that no
  longer exists. A Dir has no single "first file" a reviewer would
  recognize as what they annotated. Both fallbacks reintroduce the same
  unsound synthetic-anchor problem this ADR removes the `(1, 1)` fallback
  for, just relocated to a different wrong line instead of line 1.
- **Give Dir rows a real per-directory `AnnotationMarkers` aggregate now**,
  matching `Badges`' bottom-up shape. Considered, but deferred: `Badges`
  is baked once at tree-build time from an immutable `Report`
  (`tree/mod.rs`'s own doc comment on why baking works for `Badges` but
  not for review state), so a directory annotation counter would need
  its own bottom-up recompute pass over the tree on every
  `AnnotationMarkers` rebuild — meaningfully more machinery than the
  existing flat `HashMap<String, usize>` `file_counts`/`symbol_counts`
  use. Worth doing once a second consumer or reviewer feedback asks for
  it; not justified for this PR's budget alone.
- **Route Section/TestGroup rows to the nearest enclosing File.**
  Considered for symmetry with "every row is annotatable somehow", but
  rejected per Decision 4: it silently attaches a note to a path the
  reviewer's cursor was never actually on, and (for `TestGroup`) the
  identical file is already one row away as a directly annotatable
  `File` target, so the fallback adds an implicit rule for no real gain
  in reachability.

## Consequences

- `AnnotationLocation`/`SelectionSnapshot` gain a required
  `AnnotationTarget` field; every existing construction site (test
  fixtures included) must set it, a one-time mechanical update.
- `render_review_comments` no longer has an unsound `(1, 1)` fallback —
  every `RenderedComment` it produces now has a real anchor or range;
  annotations without one are routed to the "Additional notes" body
  section by `partition_for_export` instead, fixing the latent 422 risk
  ADR 0048's own comment on that fallback already flagged.
- The GitHub review body posted by sink A is no longer a fixed constant
  (`REVIEW_SUMMARY`) whenever at least one unanchored annotation exists
  in the batch — it becomes `REVIEW_SUMMARY` plus the rendered
  "Additional notes" section, composed once in `perform_export`.
- Dir-row tree badges, and Source-screen compose, remain unimplemented —
  explicit future work, not silently dropped scope (see PR body's Future
  work section for the concrete next steps).
- No change to sink A's underlying `gh api .../reviews` call shape or to
  `ReviewSubmitter`'s trait signature — only the `comments`/`body` values
  `perform_export` passes to it change.
