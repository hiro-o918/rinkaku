# 0045. Place a mixed file's `TestGroup` child at its source position, not always last

- Status: accepted
- Date: 2026-07-14
- Amends: [ADR 0043](0043-tui-risk-oriented-visual-encoding.md) decision 1
  (the "appended after production symbols" placement only — the fold
  itself, its collapsed-by-default state, the `N tests` label, and the
  removed per-symbol `test` badge are all unchanged)
- Related: [ADR 0027](0027-diff-pane-follows-symbol-selection.md)
  (symbol-selection auto-scroll, which assumes the diff pane's section
  order and the entry tree's row order agree), [ADR 0030](0030-diff-scroll-syncs-tree-selection.md)
  (diff-pane-to-tree scroll sync, the reverse direction of the same
  assumption)

## Context

ADR 0043 decision 1 folds a mixed file's test symbols into one
`NodeKind::TestGroup` child, appended **after every production symbol**
in that file's `children`, regardless of where the test symbols actually
sit in the source file. The diff pane (`crate::diff_shape`), by
contrast, always renders a file's hunks in source order (unchanged by
this ADR — see the Decision section below) and ADR 0027 auto-scrolls the
diff pane to a selected row's section start.

For a file that interleaves production and test code — a common Rust
shape is one or more `#[cfg(test)] mod tests` blocks positioned after
some functions but before others, or a file with test helpers threaded
between production functions — the tree's fixed "TestGroup always last"
placement can put the group node *above* production symbols that
actually appear later in the file. Moving the cursor down through the
tree's rows (production symbol, production symbol, ..., `TestGroup`,
next production symbol) does not correspond to moving down through the
file (since the `TestGroup` row, wherever it sits in the file, is always
last in the tree). The diff pane's auto-scroll offset then moves
non-monotonically as the cursor descends the tree, undoing the
"moving the cursor visibly scrolls the pane" property ADR 0027
established.

`TestGroup` starts collapsed by default (`Nav::new_collapsing_test_groups`),
so this is not visible on every keystroke — collapsed, the group
contributes exactly one row, and jumping into/out of a collapsed group
is still just one non-monotonic step, not a sequence of them. It
matters at exactly two points: navigating the cursor onto the (still
collapsed) `TestGroup` row itself, and — more visibly — after expanding
it, where every contained test-symbol row now also sits at the tree's
fixed last-child position instead of near the production siblings it
sits between in the file.

## Decision

**Change `TestGroup`'s insertion position from fixed-last to
source-order.** `build_file_node` (`rinkaku-tui/src/tree/mod.rs`)
computes the `TestGroup` node's position among a file's `children` from
the earliest test symbol's `ExtractedSymbol::range.start` (the same
new-side line the diff pane's hunks are keyed by), compared against each
production symbol's own `range.start`: the group is inserted immediately
before the first production symbol whose `range.start` is greater than
the minimum test-symbol `range.start`, or appended last when every
production symbol's range starts earlier (the common case — a trailing
`#[cfg(test)] mod tests` block, which is why the previous fixed-last
placement looked correct for most of this repository's own files).

The group itself is unaffected: still exactly one node per mixed file
(a file with test symbols scattered across multiple non-contiguous
blocks still folds into a single group, positioned at its *earliest*
test symbol's line — not one group per contiguous block), still starts
collapsed, still renders the `N tests` label, still carries none of the
removed per-symbol `test` badge. Only *where* that one node lands among
its siblings changes.

`ExtractedSymbol::range` is read inside `build_file_node`/`insert_file`
only for this ordering decision — it is not threaded onto `SymbolRef`
(the tree's public per-symbol view-model), since no other consumer of
`SymbolRef` needs a line number and adding one would widen that type's
public shape for a single internal call site.

## Alternatives

- **Do nothing (keep fixed-last placement)**: rejected — this is the
  status quo the non-monotonic auto-scroll problem described above
  comes from.
- **Revert ADR 0043 decision 1 entirely (un-fold `TestGroup`, restore
  ADR 0035's inline per-symbol `test` badge)**: considered first, since
  it would also fix the ordering problem (nothing to place out of order
  if there is no separate group node). Rejected: ADR 0043 decision 1's
  own motivation (a file with a dozen `#[test]` functions floods the
  tree with individually-badged rows) is a real, independent problem
  from the ordering complaint this ADR addresses, and reverting it would
  reintroduce that flooding to fix a narrower issue. The two properties
  — "test rows don't flood the tree" and "row order tracks source
  order" — are not in conflict once the group's *position*, not its
  *existence*, is what moves.
- **Split into multiple `TestGroup` nodes, one per contiguous block of
  test symbols**: considered, since it would make every group's position
  exactly track its block's location with no single-earliest-symbol
  approximation needed. Rejected: multiplies the row-flooding problem
  ADR 0043 decision 1 was written to solve back up by however many
  disjoint test blocks a file has, and a reviewer gains little from
  seeing "3 tests" / "2 tests" / "4 tests" at three different scroll
  positions over seeing one "9 tests" at the position of the first
  block — the position of the *first* test symbol is what the diff
  pane's auto-scroll needs to agree with for the cursor step that
  selects the group row, and every row after that within an expanded
  group is a manual scroll the reviewer controls directly.
- **Key the group's position on the diff pane's shaped section offsets
  (`crate::diff_shape::hunk_start_lines`) instead of
  `ExtractedSymbol::range.start`**: rejected — this task's boundary
  keeps `diff_shape.rs`/`ui/` untouched (a parallel change is already in
  flight against that surface), and `range.start` is already the same
  new-side line basis `diff_shape.rs` itself derives its section offsets
  from, so reading it directly in `tree/mod.rs` reaches the same
  ordering without a cross-module dependency on the diff pane's shaping
  code.

## Consequences

- `build_file_node` needs each symbol's `range.start` at partition time,
  read directly from `ExtractedSymbol` before it is converted into a
  `SymbolRef` (which does not carry a line number) — an internal
  computation, not a new field on any public tree type.
- A file where every test symbol trails every production symbol (the
  common case) renders identically to before this ADR: the group still
  lands last, since "insert before the first production symbol with a
  later line" degrades to "append" when no such symbol exists.
- A file with production symbols both before *and* after its earliest
  test symbol now shows the `TestGroup` row interleaved among the
  production rows at the position its first test symbol occupies in the
  file, instead of always last — matching the diff pane's source-order
  section list and keeping the auto-scroll offset (ADR 0027) monotonic
  as the cursor descends the tree.
- No change to `NodeKind::TestGroup`'s shape, `Nav::new_collapsing_test_groups`'s
  collapsed-by-default behavior, `order/sort.rs`'s `SiblingTier::File`
  classification of a `TestGroup` node, or any rendering
  (`row_view.rs`/`ui/entry.rs`) — every consumer keyed on `NodeKind`
  alone still matches the same arm; only the position of the node within
  its parent's `children` `Vec` differs.
- No output-format (Markdown/JSON) change: this ADR is scoped entirely
  to `rinkaku-tui`'s tree-building view-model.
