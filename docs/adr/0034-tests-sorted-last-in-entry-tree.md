# 0034. Keep tests out of production directory ranking and out of the main tree

- Status: proposed
- Date: 2026-07-13
- Amends: [ADR 0016](0016-tui-crate-and-stack.md) decision 4 (default
  topological directory ordering)
- Related: [ADR 0025](0025-default-to-including-tests.md) (tests included
  in the graph by default), [ADR 0009](0009-exclude-test-symbols-from-output-by-default.md)
  (original per-language test-detection heuristics, still authoritative)

## Context

ADR 0016 decision 4 orders the entry view's sibling directories by
SCC-condensing the change graph down to directories: least-depended-on
("entry point") directories first, most-depended-on ("foundation")
directories last. That ranking is computed purely from
`report.graph.edges`, with no notion of "this edge/node is test code."

ADR 0025 then flipped the default so test symbols are included in
`report.files`/`report.graph` like any other symbol (previously they were
excluded by default and only summarized as per-file counts under
`## Tests`). Combining the two defaults produces a ranking artifact ADR
0016 did not anticipate:

- A test module/file is typically depended on by nothing else in the
  changed set (nobody `use`s a `#[cfg(test)] mod tests` from outside
  itself) — in-degree 0 — so it ranks as an "entry point" and floats to
  the *top* of the tree, ahead of the production code it exercises.
- A production directory that is exercised by many tests picks up
  inbound test→production edges. Those edges are real graph edges the
  same as any production→production edge, so they can pull a directory's
  rank around (and, transitively via `effective_ranks`' bottom-up
  minimum, an ancestor's rank) for reasons that have nothing to do with
  the *production* dependency structure ADR 0016 decision 4 was designed
  to surface.
- When a reviewer wants to look at a test's actual assertions, the
  practical path is a definition jump from the production symbol under
  review (most editors/IDEs support "go to test"/"find usages" for this),
  not scanning the entry tree top-to-bottom for the test file. The entry
  tree's ordering value is concentrated on production code; tests
  floating to the top, or shuffled in among production directories by
  rank, actively works against that.

This ADR restores "production topological order, uncontaminated by
tests" and additionally gives tests a predictable, separate home in the
tree, rather than leaving them interleaved wherever their (irrelevant)
rank happens to place them.

## Decision

Three changes, shipped together as one coherent behavior:

**1. Exclude test-derived nodes/edges from the ranking graph.**
`order::DirCondensation::build` currently condenses every
`report.graph.node`/`edge` down to directories with no filtering. It will
skip any node whose owning symbol is test code, and any edge with a
test-code endpoint, before condensing — so Tarjan/Kahn only ever sees the
production subgraph. A directory whose *entire* graph presence is test
code (e.g. a `tests/` directory, or a file whose only nodes are test
symbols) then has no entry in `rank_directories` at all, same as today's
existing "no graph presence" case (removed-symbols-only directories) —
`order_siblings` already sorts rank-less directories after every ranked
one, A-Z.

**2. Move test files/directories to a trailing "Tests" section in the
entry tree**, rather than interleaving them among production directories
even in their now-unranked, sorted-last position. `crate::tree::Tree`
gains one synthetic node, appended to `Tree::roots` after every
production root, of a new `NodeKind::Section(SectionKind)` kind (`Tests`
is the only variant for now). Its children are every file that is a test
file *in its entirety* — `LanguageSupport::is_test_path` true for the
whole path — nested under the same directory structure they would
otherwise occupy, sorted A-Z at every level (the "Tests" section is
explicitly exempted from topological ranking; there is no production
dependency story left to tell once everything under it is test code).

A **mixed** file — real (non-test) symbols alongside test symbols in the
same file, e.g. a Rust file with production code plus a `#[cfg(test)]
mod tests` block — is *not* moved. It stays in the production tree at
its ordinary path, keeping its production ranking untouched (change 1
already scrubbed the test-derived edges/nodes out of that ranking
input). Its test symbols stay as ordinary `Symbol` children of that file
node (ADR 0025's default already puts them there) but each test symbol
now renders a `test` badge (change 3) so a reviewer can tell at a glance,
without leaving the production tree, which children of a mixed file are
test code.

Building the section is a pure post-processing step in `crate::tree`
(`build_tree` partitions already-built top-level file/dir subtrees by
"every leaf under here is a test-file leaf," lifts whichever qualify
into the new `Section` node, and leaves the rest — including mixed
files — in place), not a change to `TreeBuilder`'s insertion order or to
`report` itself.

Navigation (`crate::nav::Nav`), ordering (`crate::order::order_tree`),
and rendering (`crate::row_view::entry_row_line`) all already operate
generically over `TreeNode`/`NodeKind` via `match`; each gains one more
arm for `NodeKind::Section`:
- `Nav`: a `Section` node behaves like a `Dir` for expand/collapse and
  cursor traversal (it has `children`, participates in `push_rows`'
  pre-order walk, is keyed by its synthetic path — `"__tests__"`, chosen
  to never collide with a real slash-joined file path — in the
  `collapsed` set).
- `order::order_siblings`: `Section` is not a `Dir` for ranking purposes
  (it never receives a `DirRank`) but must still sort **after** every
  ranked and unranked `Dir`/`File` at the root level specifically — the
  comparator's existing "directories before files" split gains a third
  tier ("section last") rather than reusing the dir/file boundary.
  `Section`'s own children sort A-Z regardless of `OrderMode`, ignoring
  the topological/alphabetical toggle entirely (there is nothing to
  toggle: no ranked content lives under it after change 1).
- `row_view::entry_row_line`: a `Section` row renders like a `Dir` row
  (expand marker, bold label, aggregated badges) with the fixed label
  `"Tests"` instead of a path-derived one.

**3. Render a `test` badge on individual test-symbol rows and
whole-test-file rows**, in the same color+label convention the `api:N`
contract-change badge established (PR #97): a text label plus a color,
no emoji. Concretely:
- A `Symbol` row whose `ExtractedSymbol::is_test` is `true` (only
  reachable for a symbol living in a *mixed* file per change 2 above —
  a whole-test-file's symbols never reach the production tree at all)
  renders a trailing `test` span in magenta, reusing the color the
  existing whole-test-file `[test] (N symbols)` badge
  (`row_view::test_badge_span`) already established for "this is test
  code," so the same color means the same thing everywhere in the tree.
  This requires threading `is_test` from `ExtractedSymbol` onto
  `tree::SymbolRef` (currently absent — `SymbolRef` carries
  `classification`/`removed` but not `is_test`), since ADR 0025's default
  keeps `is_test` symbols inline in `report.files` with no marker
  surviving into the tree today.
- A whole-test-file `File` row under the "Tests" section keeps its
  existing `[test] (N symbols)` badge unchanged (`test_badge_span`,
  already shipped) — this ADR does not rename or restyle it, since it is
  already the same color/label family the new per-symbol `test` badge
  adopts.
- A directory badge aggregate is **not** introduced for test counts
  (unlike `chg:`/`api:`/`ref:`, which roll up bottom-up onto directory
  rows) — every test-only subtree now lives under the "Tests" section
  where the section label itself already says "these are tests," and a
  mixed file's own row already shows its `test` badges per-symbol; a
  redundant directory-level rollup would not answer a question the tree
  shape doesn't already answer.

## Alternatives

- **Pure topological order, unchanged (do nothing)**: rejected — this is
  the status quo this ADR fixes; tests float to the top and contaminate
  production ranking, exactly the problem described above.
- **Sort-key degrade only, no section (rank exclusion + "tests sort after
  production, A-Z" without a separate tree region)**: considered and
  rejected in favor of the fuller composite decision above. Excluding
  test edges from ranking (change 1) is necessary regardless, but
  demoting tests to merely *sort last as siblings* leaves them
  interleaved throughout the tree at whatever depth their file happens
  to sit: a repository with test files scattered across many production
  directories (`src/api/handler.rs` next to `src/api/handler_test.go`,
  `src/store/db_test.py` next to `src/store/db.py`, ...) would still show
  a test file as a sibling inside nearly every expanded production
  directory, breaking the reviewer's scan of "what production code
  changed here" with a same-level interruption every few rows, just now
  always at the *bottom* of each directory's children instead of
  scattered by rank. A single trailing "Tests" section removes that
  visual interruption entirely — a reviewer scanning production
  directories never sees a test row until they are done, and choosing to
  expand "Tests" is an explicit, deliberate action instead of
  encountering tests incidentally while reading production code.
- **A `NodeKind::Dir` synthetic node instead of a new `NodeKind::Section`
  variant**: rejected — reusing `Dir` would make the "Tests" node
  eligible for a `DirRank` lookup by path and would require giving it a
  real, collision-prone path (`Dir::path` is a slash-joined file-tree
  path everywhere else in the tree); a distinct variant makes "this node
  is not part of the ranked production graph" a type-level fact `match`
  arms enforce, rather than a runtime convention (e.g. "this magic path
  string never gets a rank entry") that a future edit could accidentally
  violate.
- **Moving mixed files into the Tests section too (whole-file move
  whenever *any* symbol in it is a test)**: rejected — a mixed file's
  non-test symbols are production code with real production dependents/
  dependencies; moving the whole file out of the production tree would
  hide those symbols' contract changes from the topologically-ordered
  scan the rest of this ADR is trying to keep meaningful, and would
  split one file's row across two tree locations if a reviewer wanted
  the detail pane for its production symbols. Keeping mixed files in
  place and marking only their test-symbol children (change 3) is
  narrower and loses no information.
- **A directory-level `test:N` aggregate badge mirroring `chg:`/`api:`/
  `ref:`**: rejected for now, per change 3's own reasoning — the section
  split already answers the aggregate question for whole-test
  directories, and per-symbol badges answer it for mixed files; revisit
  only if a concrete reviewer complaint says otherwise.

## Consequences

- `tree::TreeNode::kind` gains a new `NodeKind::Section(SectionKind)`
  variant (`SectionKind` a one-member enum today, `Tests`), so every
  existing exhaustive `match` over `NodeKind` across `nav.rs`,
  `order.rs`, `row_view.rs`, `ui/detail_pane.rs`, and `crate::app` needs
  a new arm — a compile-time-enforced checklist, not a runtime risk, but
  touches more files than the ranking change alone would.
- `tree::SymbolRef` gains an `is_test: bool` field, sourced from
  `ExtractedSymbol::is_test` (already computed during extraction, just
  not previously threaded past `pipeline::classify_symbols` into the
  TUI's view-model). Every existing `SymbolRef { .. }` test fixture
  across `tree_tests/`, `row_view_tests/`, `order_tests/`, and
  `ui/detail_pane_tests/` needs the new field added.
- `order::rank_directories`/`DirCondensation::build` need `report`'s
  per-symbol test flags to filter the graph, which means threading
  either `report.files`/`report.removed`'s symbol-level `is_test`/
  test-path information into `DirCondensation::build` (it currently only
  reads `report.graph`, which has no test/non-test distinction on its
  `Node`/`Edge` types) or extending `graph::Node`/`Edge` with that
  information at the point they are built. This ADR does not mandate
  which; that is an implementation-level call for whichever approach
  avoids widening `rinkaku-core::graph`'s public shape for a TUI-only
  concern, made when the code is written.
- Navigating into "Tests" and back must not disturb the collapse state
  or cursor stability of the production tree above it — `Nav`'s existing
  path-keyed `collapsed` set and index-based cursor already generalize to
  one more sibling subtree, so this is expected to fall out of the
  existing design rather than require new state, but must be covered by
  tests (expand/collapse the Tests section, move the cursor across the
  production/Tests boundary in both directions) since it is new surface
  this change touches.
- Whole-repo outline mode (`ReportOrigin::RepoOutline`) and diff mode
  both flow through the same `build_tree`, so a whole-repo run now also
  buckets every test file into one trailing "Tests" section — consistent
  with this ADR's reasoning (test code's ranking/position value is low
  regardless of which pipeline entry point produced the `Report`), not
  special-cased.
- No output-format (Markdown/JSON) change: `Report`'s shape is untouched
  by this ADR; only `rinkaku-tui`'s view-model (`Tree`) and rendering are
  affected.
