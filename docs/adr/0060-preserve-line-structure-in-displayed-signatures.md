# 0060. Preserve line structure in displayed signatures

- Status: accepted
- Date: 2026-07-25

## Context

`extract::slice_signature` runs every extracted signature through
`normalize_whitespace` (`split_whitespace().join(" ")`), collapsing it to
a single line regardless of its original shape. This is fine for a short
function declaration, but a `struct`/`enum`/`trait`/`interface`/`class`
signature keeps its full field/variant/method list (ADR 0014's "the
fields are the contract" decision), and a long argument list or a
`where` clause can span many lines in the source. Flattened to one line,
these become a wall of text a reviewer cannot visually parse, and a
`SignatureChanged` Markdown block becomes one giant `-` line and one
giant `+` line with no visible indication of which field actually
changed.

Whitespace normalization exists for a second, independent reason (ADR
0014): `classify_symbols` compares two signature strings verbatim to
decide `BodyOnly` vs `SignatureChanged`, and a reformatting-only edit
(e.g. reflowing a struct's fields onto more/fewer lines, changing
indentation) must not register as a contract change. Today the same
normalized string serves both the comparison and the display, so this
decision only had one lever to pull.

## Decision

Split "the string used for classification" from "the string shown to a
reader":

- `slice_signature` keeps the retained text's original line structure
  (after comment/body stripping), dedented relative to the definition's
  own starting column in the source (`node.start_position().column`) —
  not the first line's own indentation as sliced text, which a
  tree-sitter node never carries: a node's text starts exactly at its
  first token, so a nested definition's first line always reads as
  column 0 in the slice regardless of how deep it actually sits in the
  file. Using the real starting column as the dedent baseline is what
  lets a nested definition's continuation lines (which do keep their
  absolute source column) get dedented back down to that depth, while a
  top-level definition's continuation lines — genuinely indented
  relative to its own body, not to an enclosing block outside the
  slice — are left untouched. `ExtractedSymbol::signature` is now a
  multi-line string for any definition whose kept range spans more than
  one line.
- `classify_symbols` normalizes both sides' signatures at comparison time
  only (`normalize_for_comparison`), so a whitespace/line-reflow-only
  change still classifies as `BodyOnly`. This is a different transform
  from the old single-line `normalize_whitespace`
  (`split_whitespace().join(" ")`) it replaces: that transform always
  inserted a space at every normalized whitespace run, so a reflow that
  introduced a line break right after a symbol character (e.g. gofmt
  wrapping a parameter list — a newline right after `(`, a position that
  had no space in the original) compared unequal to the un-reflowed form,
  a false `SignatureChanged` purely from where the normalizer chose to
  put a space back. `normalize_for_comparison` instead collapses a
  whitespace run to a single space only when both neighboring characters
  are word characters (alphanumeric or `_`), and drops the run entirely
  otherwise — a run next to punctuation carries no information the
  comparison should key on. A whitespace run between two word characters
  is still preserved as meaningful content (`a b` vs. `ab` continue to
  compare unequal). This transform is comparison-only; every display
  render site keeps the old unconditional single-line collapse (e.g.
  `render::markdown`'s `collapse_to_single_line`) so a struct like `Foo{x:
  i32}` still reads with a space after `{`.
- Markdown's `SignatureChanged` block renders a line-based diff (each
  retained line prefixed `-`/`+`) instead of one `-`/`+` line per whole
  signature, so a single changed struct field shows up as one changed
  line, not a full-text replacement. The digest renderer (`digest.rs`)
  gets the same treatment for consistency between the two Markdown
  outputs.
- Every place a signature is used as a single structural line — the
  "Depends on:" inline code span, the TUI diff-pane section anchor/
  title, the TUI review agent-packet heading — collapses the signature
  to one line at that render site instead of carrying multi-line text
  into a slot that must stay one line.
- The TUI detail pane and the agent-packet's fenced code block already
  accept arbitrary text and keep the signature's line breaks as-is.

JSON output's `signature` (and `previous_signature`) fields become
multi-line strings (embedded `\n`) instead of single-line ones — a
breaking change to the JSON output format, sanctioned here the same way
ADR 0014 sanctioned its own comment-stripping output change.

## Alternatives

- **Re-indent/pretty-print the signature at render time** from the
  flattened single-line string, instead of preserving source structure
  at extraction time: rejected — a generic pretty-printer would need
  per-language formatting rules (brace style, wrap width, where-clause
  placement) to look right, duplicating each language's own formatter
  and breaking easily on constructs the printer wasn't written for.
  Keeping the source's own line breaks is free and always matches how
  the author actually wrote it.
- **Keep signatures single-line (status quo), rely on the renderer to
  wrap at display width**: rejected — width-wrapping (ADR 0052) is
  already a renderer concern for long *single* lines, but it cannot
  recover the original field-per-line structure of a struct/class once
  it has been flattened; a wrapped single line still reads as one
  undifferentiated blob, and a whole-signature diff still can't
  localize which field changed.

## Consequences

- Struct/enum/trait/interface/class signatures — the cases ADR 0014
  already calls out as "the fields are the contract" — become readable
  in Markdown and the TUI detail pane instead of one long line.
- `SignatureChanged` diff blocks localize to the changed line(s) instead
  of replacing the whole signature, matching how a reviewer already
  reads a normal diff hunk.
- JSON consumers that assumed `signature`/`previous_signature` are
  single-line strings must be updated to handle embedded newlines — a
  breaking output-format change.
- A handful of render sites that need a single line (dependency list
  entries, TUI diff-pane section titles) now must explicitly collapse
  multi-line signatures rather than getting a single line for free;
  each such site is documented at its own collapse call.
- Known limitation: a reflow that also adds or removes a trailing comma
  (e.g. gofmt/rustfmt inserting one when a parameter list wraps onto
  multiple lines) still classifies as `SignatureChanged`, since
  `normalize_for_comparison` only touches whitespace runs and a comma is
  a real token, not whitespace. This is judged correct rather than a gap
  to close: the comma is an actual textual difference, so treating it as
  "no contract change" would risk masking a case where a comma is added
  or removed for a reason that isn't purely cosmetic (e.g. alongside a
  parameter list edit that the diff hunk happens to also reflow).
