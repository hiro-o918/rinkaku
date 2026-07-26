# 0061. Expand tabs in TUI rendering

- Status: accepted
- Date: 2026-07-26

## Context

Source lines containing tab characters lose their indentation in the
TUI's Diff screen and Source screen. A Go file — where `gofmt` mandates
tab indentation — renders every nested statement flush against the
gutter, so the block structure a reviewer relies on to read a diff is
gone.

The cause is a disagreement between two width models that both operate
on the same string:

- The terminal draws a tab as **zero cells**. `ratatui` passes a
  `Span`'s content through to the backend, which does not translate
  `\t`; the character simply occupies no column. Measured against
  `TestBackend`, the input line `" \t\t\tdeep()"` renders as
  `"│ deep()  …│"` — three levels of indentation erased.
- rinkaku's own width arithmetic counts a tab as **one cell**.
  `crate::ui::scroll`'s wrapping, truncation and split-view alignment
  all measure with `UnicodeWidthChar::width`, which returns `None` for
  `\t`, and every call site resolves that with `.unwrap_or(1)`.

So wrapping and truncation reserve one column for a glyph that draws
none. Lines wrap early, truncation markers land in the wrong place, and
the two split-view columns drift out of alignment — on top of the
indentation loss itself.

Nothing in `rinkaku-tui` expands tabs anywhere. This is a rendering-layer
defect and it is language-independent: Go is the loudest victim because
its formatter guarantees tabs, but Makefiles (where tabs are syntax), C,
and any TypeScript/JavaScript project configured for tab indentation hit
the identical bug.

The complicating constraint is syntax highlighting. `crate::highlight`
produces `TokenSpan { start, end, palette_index }` in **byte offsets
against the line's own content**. Any transformation of the content
string invalidates every offset, so a naive `content.replace('\t', "    ")`
would shift all token coloring out of registration with the text.

## Decision

Expand tabs to spaces, and remap token offsets, in a **pure function at
the rendering entry point**, so that no tab character ever reaches a
`Span`.

- Add a pure function in `rinkaku-tui/src/ui/style.rs` that takes
  `&str` + `&[TokenSpan]` and returns the expanded `String` together
  with `TokenSpan`s rebased onto the expanded string's coordinates.
  Every byte offset is mapped through the same expansion walk that
  produces the text, so token colors stay registered with the
  characters they describe.
- Route all four content-rendering paths through it: the highlighted
  diff path (`styled_content_spans`), the unhighlighted diff path
  (`plain_diff_line`, which bypasses `styled_content_spans` entirely),
  and both source-screen paths (unified and split).
- Expansion uses **tab stops of width 4**: a tab advances to the next
  column that is a multiple of 4, rather than emitting a fixed four
  spaces.

Because expansion happens before any `Span` is constructed, the existing
`unicode_width` arithmetic in `crate::ui::scroll` becomes correct with no
change — it now measures a string whose every character has a defined
width, and that width matches what the terminal will draw.

### Why true tab stops, and why 4

A tab is a *move to the next tab stop*, not an *insert N spaces*. Column
position determines how far it advances, and honoring that is what makes
the rendered line match how the same file looks in the author's editor —
which is the whole point of expanding at all.

Width 4 rather than the traditional 8 is driven by the split view.
`MIN_SPLIT_VIEW_WIDTH` is 100 columns, leaving roughly 49 usable columns
per side. At width 8, three levels of Go indentation consume 24 of those
columns and deeply nested code wraps constantly. Width 4 keeps nested
code readable in the narrower of the two layouts, and remains a
conventional editor default.

## Alternatives

**Expand only inside `gap_span`.** The minimal change: leave `content`
and the token offsets untouched, and expand tabs only in the
uncaptured-byte spans, where leading indentation usually lands. Rejected
because it is not reliably true that indentation falls in a gap — a
tree-sitter capture may begin at byte 0 of a line, which pulls the
leading whitespace inside a token's range where `gap_span` never sees
it. The premise "indentation is always in a gap" does not hold, so the
fix would be correct only by coincidence.

**Expand at ingestion, in `crate::source` and the diff parser.**
Normalizing tabs when file content and hunks are first read would place
the transformation before highlighting, so token offsets would be
generated against already-expanded text and no remapping would be
needed. Rejected because it makes the in-memory content diverge from the
file on disk: the source screen's search (ADR 0057) matches against
`source.lines`, annotation anchoring records positions in those lines,
and any future feature that reports a column or writes text back would
silently be working in expanded coordinates. Confining the
transformation to the render boundary keeps a single, honest
representation of file content in the core and treats tab expansion as
what it is — a display concern.

**Configurable tab width.** Deferred, not rejected. A fixed 4 covers the
overwhelmingly common case; a setting can be added later without
changing the shape of the function, which already takes the column walk
as its mechanism.

## Consequences

- Tab-indented source renders with correct, visible indentation in both
  the Diff and Source screens, in unified and split layouts.
- Wrapping, truncation, and split-column alignment become accurate for
  tab-containing lines, because the string being measured no longer
  contains characters whose drawn width disagrees with the measurement.
- Rendered text is no longer byte-identical to file content. Any future
  feature that needs to map a screen column back to a file column must
  invert the expansion rather than assume identity. The expansion
  function's offset mapping is the place to build that on.
- Tab width 4 is now a rendering constant; changing it, or making it
  configurable, is an amendment to this ADR.
