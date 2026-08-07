# 0070. Annotate "Other changed files" with non-symbol changed-line counts

- Status: accepted
- Date: 2026-08-08

## Context

Dogfooding rinkaku against two external repositories surfaced the same
gap from two different angles:

- A real bugfix in `swr` changed a field on a top-level `const` object.
  No function/class signature changed, so rinkaku's Change graph and
  Definitions sections had nothing to show; the file appeared only as
  a bare path under "## Other changed files".
- A Python SDK change extended `__all__` to export a new name — a
  genuine public-API change — with the same result: the file that
  carries the public surface change is indistinguishable, in rinkaku's
  own output, from a file whose only change was a comment tweak or a
  trailing-whitespace fix.

Every built-in `LanguageSupport`'s `definition_query` targets
function/class/struct-shaped definitions; top-level `const`/`var`
bindings, import statements, and Python's `__all__` list sit outside
that query by design (`import_only_changes.rs` pins the same
guarantee for import lines). `extract_changed_symbols` correctly
returns no symbols for these files — nothing here challenges that
extraction boundary. But `render_markdown`'s "Other changed files"
section then reduces every such file to a single line with no signal
about *how much* changed or *how confident* a reader should be that
skipping it is safe. A 2-line `__all__` addition and a 200-line
non-symbol rewrite of the same file currently render identically.

## Decision

Add a changed-line-count annotation to each "Other changed files"
entry in Markdown, and the equivalent data to JSON, computed from data
the pipeline already holds — no new IO, no new parsing pass.

Concretely:

- `pipeline::analyze_diff` already iterates `changed_file.changed_ranges`
  (`Vec<LineRange>`, merged and non-overlapping — see
  `merge_adjacent_ranges`) for every file it builds a `FileReport` for.
  When that file's `symbols` end up empty (the "Other changed files"
  case), sum `end - start + 1` across `changed_ranges` to get the
  file's total new-side changed-line count, and collect
  `(path, changed_line_count)` pairs alongside the existing
  `sized_files` collection (ADR 0028's pattern).
- A new pure function in `rinkaku-core` (mirroring
  `file_size::compute_file_size_bands`) turns those pairs into a new
  `Vec<NonSymbolChange>` — `struct NonSymbolChange { path: String,
  changed_line_count: usize }` — exposed as a new `Report` field,
  `non_symbol_changes`.
- `render_markdown` looks up each "Other changed files" path in this
  new field and renders:

  ```
  - src/index.ts (12 changed lines outside any definition)
  ```

  A path with no matching entry (should not occur in practice, since
  every `files_with_no_symbols` path is produced by the same loop that
  collects the pairs) falls back to the current bare-path line rather
  than panicking.
- JSON gets the new `non_symbol_changes` array as an additional
  top-level field, additive and always present (empty array when
  `files` has no symbol-less entries), matching `file_size_bands`'
  precedent rather than `fan_ins`' file-size-warning-only subset.

### Why a new `Report` field, not a field on `FileReport`

`FileReport { path, symbols }` is constructed at 6 production call
sites and 100+ test-fixture call sites across `rinkaku-core` and
`rinkaku-tui` (Rust's exhaustive struct-literal syntax means every one
would need updating for a new field). `Report` itself already carries
several derived-but-parallel `Vec`s keyed by path or id
(`file_size_warnings`, `file_size_bands`, `test_coverage`) for exactly
this reason: adding a sibling `Vec<NonSymbolChange>` keyed by `path`
costs the same ~85-site mechanical fixture churn `file_size_bands`
already paid (PR that introduced it: `f83d232`), not the ~100+-site
churn a `FileReport` field would add on top. This also keeps
`resolve_dependencies` and `partition_test_symbols` — both of which
already reconstruct `FileReport` — untouched.

### Scope: only files with zero symbols

This ADR's fix targets `render_markdown`'s "Other changed files"
section specifically — a file already covered by "## Definitions"
that *also* has an unrelated non-symbol change (e.g. an import edit
sitting next to a changed function) is unaffected: that file's
non-symbol edit stays invisible, same as today. Extending
`non_symbol_changes` to cover symbol-bearing files too is future work
(see Consequences).

### `analyze_repo` (whole-repo outline mode)

Not affected. `analyze_repo` already drops a file from `files`
entirely once its post-filter symbol list is empty (no diff, so no
"pure rename with nothing to report but still worth noting" case per
its own doc comment) — there is no "Other changed files" section in
outline mode and no `changed_ranges` concept to measure from, so
`non_symbol_changes` is always empty for `ReportOrigin::RepoOutline`.

### Digest output

Not affected. `render_digest` only iterates `file.symbols` per file
and has no per-file "nothing to show" branch at all — a symbol-less
file already contributes nothing to the digest, and this ADR does not
change that; a digest line for a non-symbol change is future work if
it turns out to matter in practice.

### TUI

Not affected in this PR. `rinkaku-tui`'s tree already renders a
symbol-less `FileReport` as a childless `File` node with zero badges
(`tree/mod.rs`'s `build_tree` doc comment) — there is no
"Other changed files"-shaped section to annotate the same way
Markdown's is. Surfacing `non_symbol_changes` there (e.g. a badge on
the file row) is left as follow-up; `Report` gaining a field that TUI
doesn't yet consume is normal and already true of several existing
fields.

## Alternatives considered

1. **Promote top-level bindings/imports to lightweight symbols.**
   Would make these changes fully visible (signature, dependencies,
   Change graph placement) rather than just a line count, but expands
   every `LanguageSupport`'s `definition_query` and signature-slicing
   rule, likely doubling "Other changed files" noise for the common
   case (an import reordered by a formatter) that this ADR is not
   trying to surface. Deferred — worth a dedicated ADR if a concrete
   need for full symbol treatment (dependency edges into a changed
   `const`, e.g.) shows up.
2. **Do nothing — leave the file un-annotated.** Rejected: both
   dogfooding examples were real, user-relevant changes that a
   reviewer skimming rinkaku's output would have missed entirely with
   no hint that anything worth checking was there.

## Consequences

- Markdown: "Other changed files" entries change shape (additive
  suffix); existing snapshot-style tests pinning the old bare-path
  line (`empty_and_ordering.rs`, `sections_skipped_fan_in_filesize.rs`
  precedent) are updated as part of this change, since the changed
  wording is this ADR's intended effect, not an accidental
  regression.
- JSON: new always-present top-level `non_symbol_changes` field;
  every existing field's shape and every existing key's value are
  unchanged.
- `pipeline_tests`: an import-only-change scenario (already covered by
  `import_only_changes.rs` at the `analyze_diff`/`FileReport` level)
  gains an assertion that `Report.non_symbol_changes` carries the
  expected count for at least one representative language.
- Future work, explicitly deferred rather than folded into this PR:
  symbol-bearing files with additional non-symbol edits, TUI badge
  surfacing, and digest-format inclusion.
