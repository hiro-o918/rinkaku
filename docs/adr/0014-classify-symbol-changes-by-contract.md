# 0014. Classify changed symbols by contract impact

- Status: accepted
- Date: 2026-07-12

## Context

Current output reports only *which* symbols were touched. A
body-only edit to `HandleFoo` and a signature change that adds a
parameter render identically. The review-risk signal reviewers
actually want is fan-in x contract change — "a struct used from three
places had a field added" — and reviewers have separately asked for
the *location* of the change inside the signature to be visible, not
just the fact that it changed.

No symbol-level added/removed concept exists today. File-level
`ChangeKind` (Added/Modified/Deleted/Renamed) exists in `diff.rs`, but
`analyze_diff` (`pipeline.rs`) only special-cases `Deleted` files and
merges the rest; base-side content of changed symbols is never read.

The infrastructure to fix this is already in place. `main.rs` already
batch-reads git blobs via `git cat-file --batch`
(`read_git_show_files_batch`) to build the dependency index, and
extraction (`extract_all_symbols` in `extract.rs`) is a pure function
`(source, lang) -> symbols` that applies unchanged to base-side
content. Language detection is extension-based and content-
independent, so no new IO primitive is needed to parse a file's
previous revision.

Signature slicing today excludes function/method bodies, so a
body-only edit already leaves the signature string unchanged; struct
/ enum / trait / interface signatures deliberately include fields,
variants, and method specs — that inclusion *is* the contract.
Whitespace is normalized, but comment nodes inside a declaration are
kept, so a comment-only edit inside e.g. `type FooRequest struct` would
falsely register as a signature change unless comments are stripped
first.

## Decision

Classify every changed symbol as one of: `added` (absent on the base
side), `signature-changed`, `body-only`. Symbols present on base but
absent on head are reported separately as `removed`.

Detection: for each changed file, read the base-side content at the
existing IO boundary (the batch blob reader already used for the
dependency index; core stays pure — base content is passed in as
plain data), run the same pure `extract_all_symbols` on it, and
compare normalized signature strings of symbols matched by name (plus
container, e.g. a method matched within its receiver type or class).
Comment nodes are stripped during signature slicing so a comment-only
edit does not register as a contract change — this also cleans up
signatures shown today.

Data model: the change classification and the previous signature (for
`signature-changed`) live on the extracted symbol. `removed` symbols
have no head-side symbol to attach the field to, so they get a new
report-level list instead.

Rendering (Markdown): tree rows carry a compact marker for `added` and
`signature-changed` (`body-only` stays unmarked since it is the
common case and adding a marker to most rows would defeat the point).
`## Definitions` entries for `signature-changed` symbols show an
old-to-new signature diff in a ` ```diff ` fenced block. A new
`## Removed symbols` section lists the `removed` list. Hotspots
entries (ADR 0013) carry the `signature-changed` marker so "widely
used and contract changed" sorts to the top of that ranking.

JSON: classification and previous signature serialize as new,
additive fields on the symbol; `removed` is a new top-level array.

## Alternatives

- **Infer contract changes from diff hunk positions** (changed lines
  overlapping the signature's line range) without parsing the base
  side: cheap, but wrong for reformatted or reflowed declarations, and
  cannot produce the old signature text needed for the diff display.
- **Structured/AST-level signature comparison** (e.g. semantic
  equivalence that ignores parameter reordering): more precise, but
  far more per-language implementation work. Normalized-string
  comparison on signatures that are already normalized for display is
  proportionate for v1.
- **LSP-based semantic diffing**: deferred by the resolution strategy
  in ADR 0003 for the same reasons — too slow and fragile as a default
  for arbitrary checkouts. The same tags-first, LSP-later posture
  applies to change classification.
- **Skip `removed` in v1**: a removed symbol is the sharpest possible
  contract break for its callers, and base-side extraction produces
  the `removed` list for free once base-side parsing exists for
  `signature-changed` detection. Not worth deferring.

## Consequences

- Reviewers and LLMs can rank attention by contract impact; combined
  with fan-in (ADR 0013) this produces the intended risk ordering
  instead of a flat list of touched names.
- Cost: one extra parse per changed file (base side only, not a full
  repo re-index). Files with `ChangeKind::Deleted`, currently
  special-cased and skipped, can now surface their symbols as
  `removed` instead of being silently dropped — whether to fold that
  in immediately or as a follow-up is an implementation-time decision.
- Renames are not detected: a renamed symbol reports as `removed` +
  `added` rather than a single rename event. Accepted for v1; revisit
  if this proves noisy in practice.
- Stripping comment nodes changes existing signature strings shown in
  output — another pre-1.0 output-format change, consistent with prior
  ADRs (0008, 0009, 0012).
