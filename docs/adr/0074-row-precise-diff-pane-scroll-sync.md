# 0074. Diff-pane scroll sync resolves rows, not whole hunks

- Status: accepted
- Date: 2026-08-15

## Context

ADR 0072 replaced the diff pane's per-symbol section grouping with a flat,
original-order hunk list, and re-expressed both directions of the
cursor<->scroll sync in terms of whole hunks:

- forward (ADR 0027), `section_start_line_for_symbol` returned the header
  row of the **first hunk whose new-side extent intersects** the selected
  symbol's range;
- reverse (ADR 0030), `symbol_id_for_scroll_line` found the hunk whose
  rendered span contains the scroll offset and returned the **first symbol
  (source order) whose range intersects that hunk**.

Dogfooding the very next PRs showed both rules collapse as soon as one hunk
covers more than one symbol — which is not an edge case but the common
shape:

- **A new file arrives as exactly one hunk.** Running rinkaku over commit
  `870f1a4`'s own diff, `rinkaku-core/src/extract/definition_span.rs` is a
  new 186-line file: one `@@ -0,0 +1,186 @@` hunk, twelve reported symbols,
  and **all twelve resolved to scroll offset 0**. Moving the tree cursor
  down the signature list moved the diff pane nowhere; the pane sat pinned
  to the top of the file while its own pinned header named a symbol 150
  lines further down. The same diff shows the pattern four more times
  (`extract_tests/typescript.rs`, `extract_tests/python.rs`,
  `extract_tests/rust.rs`, `pipeline_tests/decorator_attribute_span_regression.rs`).
- **Ordinary hunks with context do the same on a smaller scale.** Any hunk
  whose context lines reach into a neighbouring definition covers two
  symbols, and both then share one target.
- **The reverse direction is stuck symmetrically.** Every row of that
  186-line hunk resolved to the *first* symbol intersecting it, so scrolling
  through the pane could never hand the tree cursor any symbol but the
  file's first one.

ADR 0072 itself rejected an alternative for "reintroduc[ing] the exact
'moving the cursor produces no visible motion between adjacent symbols in
the same file' complaint ADR 0027 was written to fix" — the whole-hunk
resolution rule reintroduced it anyway, through the granularity of the
lookup rather than through the shape of the content.

ADR 0072 gave a reason for choosing whole-hunk granularity: "the scroll
target a symbol selection lands on is a hunk's *header* row (no line number
of its own), so a per-line lookup would fail to resolve the exact row
auto-scroll just placed the reviewer on". That reason is real, but it is
circular — the header row is only the landing spot *because* the forward
direction resolves whole hunks. Changing both directions together dissolves
it.

## Decision

**Both directions of the cursor<->scroll sync resolve at rendered-row
granularity.** Every row of the diff pane's scrollable body carries the
new-side line coordinate it sits at, and both lookups are expressed against
that coordinate instead of against a hunk's extent.

**`crate::diff_view::new_side_positions` defines the coordinate.** An
`Added`/`Context` row sits *on* its own new-side line and takes that number.
A `Removed` row has no new-side line of its own and takes the line it
immediately **precedes** — the same "a deletion is a position, not a range"
reading `hunk_intersects` already applies to a whole pure-deletion hunk,
expressed from the following side rather than the preceding one so that a
`-`/`+` replacement pair shares one coordinate. That sharing is the point:
a changed signature's old and new lines resolve alike, so a symbol's scroll
target lands on the `-` line rather than one row past it, and the old
signature stays on screen.

**A hunk's `@@` header row shares its first body row's coordinate**
(`crate::diff_view::hunk_header_position`): the hunk's new-side start, or —
for the zero-width `(position, position - 1)` pair a pure-deletion hunk
carries — `position + 1`, the line its deleted content precedes. A symbol
that starts exactly where its hunk starts therefore still resolves to the
header row, unchanged from ADR 0072, so the `@@` line stays visible wherever
it genuinely is the start of what was selected.

**`section_start_line_for_symbol` is renamed to
`scroll_target_line_for_symbol`** and returns the first rendered row (render
order) whose coordinate falls inside the selected symbol's `LineRange`. ADR
0072 kept the old name "for continuity with ADR 0027/0030's naming"; with
the result no longer being any hunk's start line, the name now describes
something the function does not do.

**`symbol_id_for_scroll_line` resolves the row at the scroll offset to its
coordinate and returns the first symbol (source order) whose range contains
it.** Source order is retained as the tie-break for nested ranges — a
container starts before its members, so the container wins, which is what
keeps the two directions round-trip consistent. A row with no coordinate of
its own (the blank separator between two hunks, or a hunk whose header the
parser could not read) resolves to the nearest preceding row that has one,
preserving ADR 0072's "a scroll position parked on a separator belongs to
the hunk above it" boundary. An overscroll past the last row clamps to that
row, preserving ADR 0030 decision 3's open-ended span for the final hunk.

**One layout walk backs all three consumers.** `diff_shape`'s private
`diff_rows` emits one `DiffRow` per rendered row — `Separator`, `Header`, or
`Body`, each carrying its coordinate — and `hunk_start_lines` (`]c`/`[c`),
`scroll_target_line_for_symbol`, and `symbol_id_for_scroll_line` all read
it. This replaces ADR 0072's `walk_hunks`, which the three shared for the
same reason: a change to `diff_pane_lines`/`diff_pane_split_rows`'s rendered
layout has exactly one place to be mirrored.

**Nothing else changes.** `DiffPaneContent` keeps ADR 0072's flat
`Vec<AttributedHunk>` shape, the pane renders the same rows in the same
order, `]c`/`[c` still stop at hunk boundaries, and the header's `range:`
line still covers the whole file. This ADR narrows *where a selection lands
inside* that content; it does not touch the content itself.

## Alternatives

- **Reintroduce per-symbol sections or sub-hunk splitting (revert ADR
  0072).** Rejected: the reading-friction problems ADR 0072 documented
  (module-level bucket detached from the file order, decorator lines severed
  from their definitions) are real and were never symptoms of the *lookup*
  granularity. The diff pane's content ordering and its scroll-target
  resolution are independent choices, and only the second one was wrong.
- **Keep whole-hunk resolution and split only very large hunks.** Rejected:
  a threshold ("split hunks over N lines") is an arbitrary knob that leaves
  the defect in place under the threshold, and re-creates ADR 0053's
  ownership-attribution problem for the hunks it does split — the problem
  ADR 0072 deleted `hunk_split` to escape.
- **Give the reverse lookup the narrowest containing range instead of the
  first in source order.** Rejected for now: it reads better for nested
  container/member pairs, but it breaks round-trip consistency (a
  container's own forward target can land on a row a member also contains,
  so the reverse lookup would hand the cursor back a different symbol than
  the one that scrolled there) — and the feedback-loop guard ADR 0030
  decision 6 documents depends on the two directions agreeing. A separate
  change could revisit this by having the *caller* prefer the currently
  focused symbol, which needs no round-trip property from `diff_shape`.
- **Have the forward direction back up to the enclosing hunk's header for
  context.** Rejected: it reintroduces the same collapse for every symbol
  whose first row sits within a screenful of the hunk header, and the
  information a header carries (old/new line numbers) is already in the
  pane's own pinned `range:` header line.

## Consequences

- Selecting a symbol scrolls the diff pane to that symbol's own first
  changed line, including inside a hunk shared with its neighbours. The
  `@@` header of that hunk scrolls off the top unless the symbol starts
  where the hunk does — the deliberate trade for the pane actually tracking
  the signature list.
- Scrolling the diff pane hands the tree cursor each symbol in turn as the
  scroll offset passes into its lines, rather than pinning the cursor to a
  shared hunk's first symbol.
- The two directions round-trip: a symbol's auto-scroll target resolves back
  to that same symbol. Verified across commit `6933471`'s 58-file diff — 211
  reported symbols, 211 round-trips, no unresolved targets — and pinned by
  `diff_shape_tests::symbol_id_for_scroll_line::should_round_trip_the_scroll_target_of_every_symbol_under_one_hunk`.
- `crate::diff_view::hunk_intersects`/`hunks_for_range` are no longer used
  by `crate::diff_shape`; they stay for `crate::review_flow`'s annotation
  anchoring and `crate::highlight`, which ask a genuinely whole-hunk
  question ("which hunks touch this symbol at all"). The one behavioural
  difference between the two rules is a pure-deletion hunk sitting
  immediately before a symbol's first line: `hunk_intersects`' half-open
  rule excludes it, the row rule includes it. Including it is the better
  answer now that ADR 0073 folds decorators and attributes into a
  definition's span — content deleted directly above a definition is
  usually that definition's own.
- `crate::ui::diff_pane::new_side_line_numbers` (ADR 0048's annotation
  markers) is now derived from `new_side_positions` rather than walking the
  hunk itself, dropping `Removed` rows back to `None`: an annotation must
  anchor to a line that exists in the new file, which a deleted line does
  not.
- No change to `rinkaku-core` or to Markdown/JSON output — scoped entirely
  to `rinkaku-tui`'s presentation layer, like every ADR it amends.

## Amendment history of amended ADRs

- **Amends ADR 0072**: the two scroll-sync rules ("auto-scroll to the first
  hunk intersecting the symbol's range", "resolve a scroll position by
  whole-hunk intersection") narrow to their row-precise forms, and
  `section_start_line_for_symbol` is renamed. ADR 0072's content decisions —
  flat original-order hunk list, no sections/contract header/module-level
  bucket, `hunk_split` deleted, whole-file `range:` header — are unaffected.
- **Amends ADR 0027**: decision 2's auto-scroll target narrows from "the
  selected symbol's first intersecting hunk" (ADR 0072's wording) to "the
  selected symbol's own first rendered row". Decisions 4-7 are unaffected.
- **Amends ADR 0030**: the reverse lookup's granularity changes; decision 3
  ("leave the tree cursor untouched rather than guess" when nothing
  resolves) and decision 6 (the feedback-loop guard) are unaffected.
