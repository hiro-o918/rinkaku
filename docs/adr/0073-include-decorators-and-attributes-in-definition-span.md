# 0073. Include decorators and attributes in a definition's span

- Status: accepted
- Date: 2026-08-11
- Relates to: [ADR 0071](0071-slice-container-signatures-to-touched-lines.md)
  (the untouched-member removal this decision's extended span also
  feeds into)

## Context

A Python `@decorator` and a Rust `#[attribute]` both sit outside the
tree-sitter node `definition_query` captures for the definition they
annotate:

- Python: `function_definition`/`class_definition` is wrapped in a
  `decorated_definition` node once any decorator is present; the query
  captures the inner node directly, not the wrapper (see
  `language/python.rs`'s `DEFINITION_QUERY` doc comment).
- Rust: an `attribute_item` (`#[derive(...)]`, `#[allow(...)]`, ...) is
  a preceding *sibling* of the item it annotates, not a child, and
  `definition_query` never walks siblings to include it (see
  `language/rust.rs`'s `has_test_attribute`, which already performs
  this walk for a different purpose: test-attribute detection).

Consequently, a diff that touches only a decorator/attribute line —
adding `@dataclass`, changing `#[derive(Debug)]` to
`#[derive(Debug, Clone)]` — is invisible to rinkaku: the changed line
falls outside every reported definition's row range, so
`extract_changed_symbols` reports nothing, and the definition's
signature (when it *is* reported for some other reason) never includes
the decorator/attribute text.

TypeScript does not have this problem, but not by design: its grammar
attaches a decorator as a child of `class_declaration` itself
(`(decorator) (class) name: (type_identifier) ...`), so it is already
inside the captured node's row range and already inside the text
`slice_signature` keeps. This is a grammar-shape accident, not a
deliberate choice recorded anywhere — Python and Rust's current
"decorator-blind" behavior is the actual v1 simplification (Python's
`DEFINITION_QUERY` doc comment says so explicitly), and this ADR closes
that gap so all three languages behave the same way.

## Decision

Add one method to `LanguageSupport`:

```rust
fn definition_span_start<'a>(&self, node: Node<'a>) -> Node<'a> {
    node
}
```

Default implementation returns `node` unchanged — Go, TypeScript, and
HCL keep exactly today's behavior with zero code. Python and Rust
override it:

- **Python**: if `node`'s parent is `decorated_definition`, return that
  parent. This also covers a class method's own decorator, since
  `decorated_definition` wraps a nested `function_definition` the same
  way it wraps a top-level one (verified against the grammar: a
  decorated method inside a class body is `block ->
  decorated_definition -> function_definition`, structurally identical
  to the top-level case).
- **Rust**: walk backward through `node`'s preceding siblings,
  extending across consecutive `attribute_item` nodes, and return the
  earliest one found (or `node` itself if there are none). The walk
  stops — does not extend across — a `line_comment`/`block_comment`
  sibling: a comment sitting between an attribute and its item is
  common (a doc comment describing why the attribute is there), and if
  the span extended through it, a comment-only line edit would make the
  whole definition register as touched, which is not this ADR's goal.
  This intentionally diverges from `has_test_attribute`'s sibling walk
  (`is_skippable_between_attribute_and_item`), which *does* skip over
  comments — that walk answers "is this item test-attributed at all",
  where a comment in between is irrelevant to the yes/no answer; this
  walk instead answers "which lines belong to this definition", where
  a comment in between is content that must stay outside the span for
  the pin above to hold.

`with_definition_nodes` (`extract/mod.rs`) computes each captured
node's extended span once, right after the query match, and wraps the
result in a small value type (`extract::definition_span::DefinitionNode`)
carrying the original node plus the span's start byte/row/column. Every
downstream consumer that today reads a bare node's own
`start_byte()`/`start_position()` for range or slicing purposes reads
the `DefinitionNode`'s span-start instead:

- the touched/overlap check (`node_to_line_range`, feeding
  `extract_changed_symbols`'s filter)
- `ExtractedSymbol::range`
- `slice_signature`'s declaration-prefix and class-header start (so a
  decorator/attribute is included in the signature text, not only in
  the touched-range check)
- ADR 0071's untouched-member removal (`container_slice.rs`): a
  member's widened removal range now starts at the member's own
  decorator/attribute, so dropping an untouched method also drops its
  decorator.

`collect_referenced_names` (`extract/references.rs`) is **not**
changed — it is still called with the original (unwidened) node, so a
Python decorator or Rust attribute — sitting outside that node's own
subtree — contributes nothing to `referenced_names`/
`referenced_method_names` either before or after this decision. A
decorator/attribute's own arguments (`@app.route("/x")`,
`#[serde(rename = "y")]`) are not scanned for references for these two
languages; this is deliberately deferred, not silently dropped, since
decorator arguments are a distinct, lower-confidence reference shape
(often configuration values, not symbol names) that deserves its own
decision if it turns out to matter in practice. TypeScript already
diverges from this by construction and is unaffected: its decorator
sits *inside* `class_declaration`'s own subtree (the same grammar
accident this ADR's Context section describes), so `Component` in
`@Component() class Widget {}` was already collected as a reference
before this decision and continues to be.

## Alternatives

- **Widen the tree-sitter query itself** (capture
  `decorated_definition`/a synthetic attribute-inclusive span directly
  in `DEFINITION_QUERY`): rejected for Rust — there is no single Rust
  grammar node spanning an attribute plus the item it annotates (they
  are siblings, not parent/child), so the query language cannot express
  this; a post-query widening step is required regardless, and reusing
  the same mechanism for Python keeps the two languages' handling
  symmetric instead of one being query-based and the other
  procedural.
- **Only extend the touched-range check, leave `slice_signature`
  reading the bare node**: rejected — a decorator-only change would
  then be *detected* but the reported signature would still omit the
  decorator that changed, which is the same "hides the actual edit"
  problem this ADR exists to fix, just moved one step later.
- **Keep `with_definition_nodes` returning bare `Node`s and recompute
  the span start at each call site**: rejected — every call site would
  need its own `lang.definition_span_start(node)` call and the same
  per-node tree walk repeated (once per touched-range check, once for
  `range`, once for `slice_signature`, once per container-slice
  member); computing it once in `with_definition_nodes` and passing a
  small struct is cheaper and removes the chance of one call site
  silently reading the un-widened node.

## Consequences

- A decorator/attribute-only change is now detected as
  `SignatureChanged` (base and head both slice the same way, so the
  comparison stays symmetric) instead of being invisible.
- A reported symbol's signature now includes its decorator(s)/
  attribute(s) even when the diff that triggered the report touched
  only the `def`/`class`/`fn`/`struct` line itself, not the
  decorator — this widens what today's tests pin as the expected
  signature text for every decorated/attributed Python and Rust
  fixture, a deliberate, one-time output-shape change accepted as part
  of this ADR (not something call sites need to opt into).
- A Python class method's decorator-only change now surfaces the
  method itself as the narrowest touched definition (per the existing
  narrowest-enclosing-definition rule), not the whole enclosing class —
  previously neither was reported at all, so this is a strict
  improvement, not a behavior this ADR needs to choose between.
- `symbol.range` for a decorated/attributed symbol now starts at the
  decorator/attribute line, not the `def`/`fn` line — this is
  serialized in JSON output and consumed by `rinkaku-tui` for
  highlighting; both already treat `range` as "the symbol's full
  extent" and need no code change, only wider highlighted spans for
  decorated symbols.
- Rust's `#[test]`/`#[rstest]`/`#[cfg(test)]`-style attributes are
  ordinary `attribute_item` siblings under this mechanism, so a test
  function's reported `range` now also starts at its test attribute
  line — `is_test_definition`'s own detection logic is unaffected (it
  inspects the node's siblings directly, independent of
  `definition_span_start`), only the reported range/signature text
  shifts.
- **Deferred, not fixed by this decision**: the existing gap where a
  diff touching both a container's own body-level line and one of its
  members in the same hunk set still suppresses the container in favor
  of the member (ADR 0071's Consequences) is unchanged — a decorator on
  the *container itself*, changed alongside a member change, remains
  subject to that same pre-existing suppression rule, not a new gap
  this ADR introduces.
