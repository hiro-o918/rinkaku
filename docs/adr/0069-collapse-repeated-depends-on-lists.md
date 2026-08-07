# 0069. Collapse repeated "Depends on" lists in Markdown Definitions

- Status: accepted
- Date: 2026-08-08

## Context

Dogfooding PR #237's diff (`rinkaku --base 3c5ecf7 --head d8b810f`)
showed the `## Definitions` section repeating the same `Depends on:`
list verbatim across sibling symbols: 7 of the 11 test functions in
`rinkaku-core/src/extract_tests/classification.rs` share an identical
set of helper/type dependencies (`Classification`, `LineRange`,
`RemovedSymbol`, three same-named `symbol` helpers, and the
`(+26 more definitions matched by name)` note), so the same ~7-line
block is printed 7 times in a row.

The "Change graph" tree already solves the analogous problem for
shared subtrees: `render_tree_node` prints a node in full only once
and every later occurrence as a one-line `(see above)` reference (ADR
0008, condensed further by ADR 0012). `render_definition`'s `Depends
on:` list has no equivalent — each symbol's dependency list is
rendered independently of what came before it in the same document.

## Decision

In `render_markdown`, before rendering `## Definitions`, track, per
file, the most recently rendered non-empty `dependencies` list (using
`ExtractedSymbol::dependencies`' derived `PartialEq`, list order and
all). When a symbol's `dependencies` list exactly equals the tracked
list for its own file, `render_definition` emits one reference line
instead of repeating the list, pointing at the *nearest* earlier match
(not necessarily the first occurrence in the file) so the reference is
always as close by as possible:

```
Depends on: same as `fn should_classify_as_added_when_no_base_side_match_exists` above
```

Design choices, narrower than the tree's collapse:

- **Exact match only.** No near-identical merging (e.g. subset/superset
  folding) — that would drop information a reader might need, unlike
  the tree's `(see above)`, which never loses a name (the full list is
  still under `## Definitions` for the first occurrence). A collapsed
  `Depends on:` line still points at a real prior entry that has the
  complete list.
- **`omitted_dependency_matches` participates in the comparison
  implicitly** by being rendered as part of the same list check: two
  symbols with identical `dependencies` but a different omitted-count
  are treated as different lists (the omitted-count line is part of
  what "the same Depends on block" means here), so the collapse never
  hides a differing "+N more" note.
- **Scoped to the same file, not the whole report.** Two unrelated
  files can coincidentally define a same-shaped dependency (or, more
  commonly, both have an empty list), and a cross-file "same as X
  above" reference would send a reader jumping between files to
  confirm a coincidence instead of an intentional shared block. Diffs
  also commonly cluster same-file test siblings next to each other
  (as in the PR #237 example), so same-file scoping already catches
  the common case without a cross-file reference's extra cognitive
  cost.
- **JSON output is unaffected.** JSON serializes `Report`/
  `ExtractedSymbol` directly; a machine consumer parsing
  `dependencies` has no volume problem and no compaction is warranted.
  `digest` output already omits dependencies entirely and stays
  untouched.
- **Empty lists are never collapsed.** `render_definition` already
  skips the whole `Depends on:` block when a symbol has no
  dependencies and no omitted matches, so there is nothing to
  reference.

## Alternatives

- **Cross-file scoping**: catches more repeats (e.g. two files with
  the same single dependency) but trades a within-file glance for a
  jump to another file's heading; rejected per PR #237's dogfooding
  example, where every repeat was already same-file.
- **Near-identical merging (e.g. collapse when lists share N of M
  entries)**: strictly more information loss for a fuzzier win; the
  observed case is exact-match repetition, so exact match already
  captures the value without the risk.
- **Deduplicate by content hash across the whole document into a
  shared "Common dependencies" appendix**: bigger restructuring of the
  `## Definitions` section for a gain the observed case does not need;
  the per-symbol reference line keeps the existing one-entry-per-
  symbol shape intact.

## Consequences

- `## Definitions` shrinks materially on diffs with repeated sibling
  dependency shapes (the PR #237 example: 7 duplicate ~7-line blocks
  become 7 one-line references) with no information loss — the full
  list remains readable at the first occurrence.
- Another Markdown-only formatting change (JSON untouched), consistent
  with the ADR 0010/0012 precedent of keeping JSON as the raw,
  uncondensed shape.
- The collapse reference names the earlier symbol by its label (same
  format as tree/heading labels), so a reader can locate it either by
  scrolling up or by search.
