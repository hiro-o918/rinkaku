# 0052. `App::right_pane_scroll` is always a pre-wrap logical-line offset; wrap conversion is confined to the renderer

- Status: Accepted
- Date: 2026-07-15

## Context

`crate::diff_shape::walk_sections` (and its derivatives
`section_start_line_for_symbol`, ADR 0027 decision 2, and
`symbol_id_for_scroll_line`, ADR 0030 decision 2) compute a symbol
section's scroll position by counting *logical* lines of the diff
pane's shaped content — one count per rendered line before any
width-based wrapping. `crate::ui::scroll::render_scrollable_pane`
consumed that same `App::right_pane_scroll` value as a *display-row*
index into `wrap_lines`/`pair_wrap`'s wrapped output, passed straight
into `clamp_scroll` and `Paragraph::scroll` with no conversion between
the two. Both sides shared one `usize` field, but the writer
(`crate::diff_shape`) and the reader (`crate::ui::scroll`) disagreed on
its unit whenever a signature line was long enough to wrap — which
`render_scrollable_pane`'s own doc comment had already flagged as a
hazard in the abstract ("any logical line long enough to wrap desyncs
the scroll unit from the rendered unit") without anyone having
actually violated it until this contract was in place on both sides at
once.

The mismatch produced two symptoms:

1. Selecting a symbol in the entry tree wrote the correct *logical*
   section-start line into `right_pane_scroll`, but the renderer
   applied that number as a *display-row* offset — in a narrow pane
   where an earlier section's signature wrapped, this landed short of
   the target section, leaving the previous symbol's wrapped
   continuation on screen instead.
2. `crate::ui`'s post-draw fold-back
   (`clamp_right_pane_scroll_after_draw`) writes the *clamped
   display-row* value straight back into `App::right_pane_scroll`
   unchanged whenever it was already in bounds (`clamp_scroll` only
   ever lowers an overshooting value, never raises one) — so the
   diff → tree reverse sync (`symbol_id_for_scroll_line`) went on to
   compare that (accidentally valid-looking, but display-row-shaped)
   number against `crate::diff_shape`'s logical-line section
   boundaries. A scroll position sitting inside an earlier symbol's
   wrapped display rows was misread as if it were several logical
   lines further along, so the reverse lookup reported a different
   (further-along) symbol than what the pane was actually showing.

The existing regression test for this scroll path (`926876e`,
`scroll_sync_tests.rs`) always used a 160-column pane against short
single-line signatures, so no wrapping ever occurred and neither
symptom had coverage.

## Decision

**`App::right_pane_scroll` is always a logical-line offset — an index
into the diff pane's shaped content *before* any width-based
wrapping.** This was already `crate::diff_shape`'s unit; the fix makes
`crate::ui::scroll` honor it instead of silently reinterpreting it.

**All wrap-width knowledge stays confined to
`crate::ui::scroll::render_scrollable_pane`.** `crate::diff_shape`
gains no notion of pane width, and `App` gains no notion of wrapping —
exactly the module boundaries both already claimed in their doc
comments; this decision is enforcing an existing (but violated)
contract, not inventing a new one.

**Wrap functions now also report each output row's originating logical
line.** `wrap_lines_with_origins`/`pair_wrap_with_origins` replace
`wrap_lines`/`pair_wrap` at `render_scrollable_pane`'s two call sites,
returning `(wrapped_rows, origins)` where `origins[i]` is the logical
line display row `i` was wrapped from. Two new pure functions convert
between the two coordinate spaces using that mapping:

- `logical_line_to_display_row(origins, logical_line)` — the first
  display row of a given logical line (used going *in*: converting
  `requested_scroll` before `clamp_scroll`/`Paragraph::scroll` run).
- `display_row_to_logical_line(origins, display_row)` — the logical
  line a given display row belongs to (used going *out*: converting
  the clamped display-row value back to logical lines before
  `render_scrollable_pane` returns it).

`render_scrollable_pane`'s contract is now: callers pass and receive
logical lines; `clamp_scroll`/`scroll_indicator`/`Paragraph::scroll`
internally still operate on display rows, but that unit never crosses
the function boundary.

**`Focus::Right` `Up`/`Down` now move by one logical line, not one
display row.** When a logical line wraps into several display rows,
`j`/`k` jumps the whole logical line at once instead of stepping
through its wrapped rows one at a time — an accepted granularity
change (`app/handle_key.rs`'s `(Screen::Entry, Focus::Right, ...)`
arms needed no code change; only their unit changed).

**Detail/BlastRadius panes are unaffected in practice.** They share
`render_scrollable_pane` but pass plain unwrapped `Vec<Line>` with no
per-symbol section concept of their own, so "logical line" there is
just "index into that `Vec<Line>`" — the same thing it already was.
The unit distinction only matters where a second party (`crate::diff_shape`)
computes positions against the same field.

## Alternatives

- **Make `crate::diff_shape` wrap-aware** (pass pane width down into
  `walk_sections` and compute display-row positions directly).
  Rejected: `crate::diff_shape` is deliberately free of `ratatui`
  types and pane-layout knowledge (its own module doc comment); this
  would leak a rendering concern into a pure view-model module and
  couple every `diff_shape` call site to a concrete pane width even
  where wrapping is not the caller's concern (e.g. `hunk_start_lines`'
  `]`/`[` jump math).
- **Store both units on `App`** (a display-row field alongside the
  logical-line one, kept in sync by the renderer). Rejected: doubles
  the state `App::handle_key`'s reset rules must track and reintroduces
  exactly the two-source-of-truth risk this fix removes; a single
  field with one unconditional unit is simpler to reason about.
- **Give `wrap_lines`/`pair_wrap` an output-only breaking change**
  (return `(Vec<Line>, Vec<usize>)` directly, dropping the old
  signature). Rejected in favor of keeping the two-tuple functions as
  thin wrappers initially, then removed once nothing outside tests
  still called them (`cargo clippy --all-targets` flagged them as dead
  code once `render_scrollable_pane` moved to the `_with_origins`
  variants) — the `_with_origins` functions are now the only public
  surface, and existing tests were updated to call them and discard
  the origins tuple element where they don't need it.

## Consequences

- `crate::ui::scroll::render_scrollable_pane` converts `requested_scroll`
  to a display row before `clamp_scroll`, and converts the clamped
  display row back to a logical line before returning — every caller
  outside this one function (`crate::app`, `crate::diff_shape`,
  `crate::event_loop::scroll_sync`) continues to read and write
  `right_pane_scroll` purely in logical-line terms, matching what their
  own doc comments already claimed.
- `wrap_lines`/`pair_wrap` (the old two-tuple/two-value signatures) are
  removed; `wrap_lines_with_origins`/`pair_wrap_with_origins` are the
  only wrap entry points, used both by `render_scrollable_pane` and by
  tests that need the plain wrapped output (discarding the origins
  return value).
- New regression tests (`rinkaku-tui/src/event_loop/scroll_sync_wrap_tests.rs`,
  `rinkaku-tui/src/ui/scroll_tests/wrap_origins.rs`) use narrow panes
  and multi-row-wrapping signatures specifically to exercise the
  logical/display-row gap `926876e`'s wide-pane fixtures could not
  reach.

## Amendment: fold-back must not undo the request (dynamic verification follow-up)

Dynamic verification of the fix above found a second-order regression it
introduced: `display_row_to_logical_line` alone is not a safe fold-back once
a *single* logical line is long enough to occupy the whole viewport. When
`clamp_scroll`'s display-row clamp lands inside that line's own wrapped
span (rather than exactly at its first row), the fold-back reports that
line's own index — silently undoing a request for a *later* logical line.
The next `Down` re-requests the same target, resolves to the same clamped
display row, and folds back to the same value: a stable fixed point well
short of the content's end (reproduced with a huge wrapped leading line
followed by short lines; `right_pane_scroll` never advanced past it).

`render_scrollable_pane`'s write-back now goes through
`resolve_folded_back_logical_line(origins, display_row, requested_scroll)`
instead of calling `display_row_to_logical_line` directly: it floors the
folded-back value at `requested_scroll` itself (capped at the last
available logical line, so an overscrolled request cannot be reported as
unclamped). The on-screen `display_row` passed to `Paragraph::scroll` is
unchanged — this only corrects the logical-line value written back into
`App`.

Regression coverage:
`rinkaku-tui/src/event_loop/scroll_sync_wrap_tests.rs`'s
`should_advance_scroll_monotonically_past_a_huge_wrapped_leading_line_*`
and `should_not_oscillate_when_alternating_down_and_up_past_a_huge_wrapped_leading_line`
drive repeated `Down`/`Up` against a huge-then-short fixture at several
viewport heights; `rinkaku-tui/src/ui/scroll_tests/wrap_origins.rs` covers
`resolve_folded_back_logical_line` directly.
