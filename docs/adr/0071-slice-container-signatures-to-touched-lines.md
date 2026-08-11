# 0071. Slice a reported container's signature to touched lines only

- Status: accepted
- Date: 2026-08-11
- Relates to: [ADR 0014](0014-classify-symbol-changes-by-contract.md)
  (comment-stripped signature comparison, whose comparison input this
  decision changes), [ADR
  0060](0060-preserve-line-structure-in-displayed-signatures.md)
  (multi-line signature slicing this decision builds on)

## Context

Python and TypeScript classes are single tree-sitter nodes whose span
covers the entire class body, methods included. When a changed line
falls directly in the class's own body (a field, a class-level
comment, the header/base-class list) rather than inside any nested
method, `extract_changed_symbols`'s narrowest-enclosing-definition rule
picks the class itself as the reported symbol, and `slice_signature`
returns the whole class text with every method body stripped — but
every method *signature* kept, touched or not.

Reproduction: changing one field's value in a Python class with
several unrelated methods —

```python
class Widget:
    label = "a"          # only this line changes, "a" -> "b"

    def move(self, dx, dy): ...
    def scale(self, factor): ...
```

— reports `Widget`'s signature as the field plus both `move` and
`scale` method signatures, neither of which changed. A reviewer (or an
LLM consuming this output) sees two unrelated methods listed as part of
"what changed" for a one-line edit.

Go and Rust do not have this problem: a Go method is a sibling
`method_declaration` node linked to its receiver type by field, not
nested inside the receiver's own node, and Rust methods live in a
separate `impl_item` node from the type they extend. Neither ever
becomes "a container node whose body text embeds sibling method
signatures," so this is a Python/TypeScript-specific (class-shaped)
issue.

### Constraint: classification must not be affected

ADR 0014's `classify_symbols` compares each reported head-side symbol's
`signature` against a matching base-side symbol's `signature`
(byte-for-byte after whitespace normalization) to decide
`Added`/`SignatureChanged`/`BodyOnly`. The base-side symbol always
comes from `extract_all_symbols`, which indexes every definition in a
file regardless of any diff and therefore has no notion of "touched
lines" to slice by — its container signatures stay whole-class,
unaffected by this decision.

Naively slicing `ExtractedSymbol::signature` for containers inside
`extract_changed_symbols` would make the head-side comparison input a
touched-lines-only fragment while the base-side stays whole-class:
even a container whose only actual change is a harmless reflow (no
token-level difference) would compare unequal on size alone, turning
what should classify `BodyOnly` into `SignatureChanged`. Classification
must keep comparing whole-class text on both sides.

## Decision

Keep two signature values in play for a reported container symbol, but
without adding a field to `ExtractedSymbol`:

1. `extract_changed_symbols` (in the new `extract::container_slice`
   module) narrows a reported container's `ExtractedSymbol::signature`
   — the field callers render — to its header plus only the body-level
   lines (member declarations, not member bodies) that overlap
   `changed_ranges`. A member definition (method, nested class, ...)
   with no overlapping line is dropped from the signature text
   entirely, header-and-all; one with an overlapping line keeps
   exactly as much of itself as the existing per-kind slicing already
   produces (e.g. a touched method keeps its own signature line, body
   still stripped).
2. `classify_symbols` already receives `all_head_symbols` — the
   complete, un-narrowed symbol set for the head file (added by the
   "container falsely reported removed" fix this ADR's context section
   above builds on) — precisely to judge container survival against
   the whole file rather than the diff-narrowed `head_symbols` list.
   The signature comparison now also looks up each head symbol's
   `(name, container)` identity in `all_head_symbols` and compares
   *that* whole-class signature against the base side, falling back to
   the (already-whole, for every non-container kind) `head_symbols`
   signature when no `all_head_symbols` entry exists. This keeps every
   classification decision comparing whole-class text on both sides,
   identical to today's comparison, while `ExtractedSymbol::signature`
   itself is free to carry the narrower, touched-lines-only text for
   display.
3. The member-vs-container distinction is expressed generically: a
   member is any `@definition`-query-captured node that is a
   descendant of the reported container node. No per-language special
   case — the same code path narrows a Python class and a TypeScript
   class identically.

## Alternatives

- **Show elided members as a placeholder line** (e.g. `# ... 2 more
  methods unchanged`): rejected for v1 — it requires deciding how to
  count/name elided members across arbitrarily nested containers
  (nested classes, class-field arrow functions) for a benefit
  (explicitly signaling "there was more here") that is secondary to
  the noise this ADR removes. Can be added later as a rendering-layer
  concern without changing the extraction contract this ADR sets.
- **Add a second field to `ExtractedSymbol`** (e.g.
  `comparison_signature`) carrying the whole-class text alongside the
  narrowed `signature`: rejected — `ExtractedSymbol` already has two
  fields serving an analogous split-purpose role
  (`referenced_names`/`referenced_method_names`, both `#[serde(skip)]`
  intermediate artifacts), and each past addition required touching
  every one of this struct's ~160 test-literal construction sites
  across the repo (see ADR 0068's Consequences). Reusing
  `all_head_symbols`, which `classify_symbols` already receives for an
  adjacent reason, avoids that repo-wide mechanical churn for a
  same-shaped problem.
- **Slice in `pipeline.rs` after classification runs, using a
  re-parse**: rejected — the touched-lines slice needs the AST node
  boundaries of each member inside the container, and
  `extract_changed_symbols`'s tree-sitter `Node` values are
  intentionally scoped to `with_definition_nodes`'s callback and never
  escape it (see that function's doc comment); re-parsing in
  `pipeline.rs` to regain node access would duplicate the parse this
  ADR's approach avoids.

## Consequences

- `ExtractedSymbol::signature` for a container reported because of a
  body-level (not nested-member) touched line now omits every
  untouched member's signature — output shape unchanged (still a
  `String`), but shorter/narrower content for this case. Renderers
  (Markdown/JSON/digest) need no changes; they already just print
  whatever `signature` holds.
- Classification (`Added`/`SignatureChanged`/`BodyOnly`) is unaffected
  by this decision for every symbol kind, containers included — pinned
  by regression tests comparing classification output before and
  after this change on the same inputs.
- **Known limitation, out of scope for this decision**: if a diff
  changes both a container's own body-level line (e.g. an attribute)
  *and* one of its methods in the same hunk set, the narrowest-
  enclosing-definition rule (unchanged by this ADR) still suppresses
  the container in favor of the touched method as the reported symbol
  — the attribute change becomes invisible to this diff's output. This
  pre-existing behavior is not something this ADR's touched-lines
  slicing introduces or fixes; addressing it would mean changing which
  symbol gets reported, not how a reported container's signature is
  sliced.
