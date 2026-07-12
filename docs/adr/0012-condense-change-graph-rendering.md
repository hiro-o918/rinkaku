# 0012. Condense change-graph rendering for human readability

- Status: accepted
- Date: 2026-07-12

## Context

Dogfooding the entry-point tree (ADR 0008, after the test/generated
exclusions of ADR 0009–0011) on a real-world Go diff that touched a
repository interface and its callers still produced output that humans
found hard to read. Three structural problems were observed:

1. **Leaf data-carrier types dominate the tree.** In request/response
   style APIs, every changed method drags in one or two changed structs
   (`FooRequest` / `FooResponse`). These render as their own nested
   lines — most of them `(see above)` repeats — so the majority of tree
   lines carry near-zero information. Over half of a 35-line graph was
   such lines in the observed diff.
2. **An interface and its methods are listed twice.** The data model has
   no link between an interface declaration and the (receiver) methods
   that implement or mirror its specs. When both change, each becomes an
   independent root: the interface appears at top level, and every
   method appears again at top level with the same struct children
   repeated as `(see above)`.
3. **The shape of the change is invisible.** A typical diff has an
   epicenter (one file where changed symbols concentrate) and a blast
   radius (callers spread across a few files), but a flat list of roots
   with a full path on every line does not convey that fan shape.

## Decision

Three rendering-layer changes, Markdown output only (JSON continues to
expose the raw graph unchanged, per the ADR 0010 precedent):

1. **Inline leaf type dependencies.** A child node that (a) is a
   non-function symbol (struct / enum / type / interface / trait /
   class) and (b) has no children of its own in the graph is not
   rendered as a nested line. Instead its name is appended to the parent
   line as an inline annotation: `- fn UpsertItems (store/items.go) —
   uses: UpsertItemsRequest, UpsertItemsResponse`. The criterion is
   structural, not name-based (`*Request` / `*DTO` conventions vary),
   so it is language-agnostic and needs no per-language tuning. No flag
   is added: unlike ADR 0009/0010 exclusions, no information is removed
   — every folded name is still visible inline and its full signature
   still appears under `## Definitions`.
2. **Link interfaces to their methods.** Each `LanguageSupport` whose
   grammar has interface-like declarations with named method specs (Go
   `interface`, TypeScript `interface`, Rust `trait`, Python `Protocol`
   left to its existing class handling) adds those spec names to the
   interface symbol's `referenced_names`. The existing name-based edge
   builder (ADR 0008) then links the interface node to same-named
   changed function nodes, which removes those methods from the root
   set and nests them under the interface. This stays inside the
   established name-based model — no new edge kind, no new symbol
   relation in the data model.
3. **Summary line under `## Change graph`.** One line stating the
   total changed-symbol count, file count, and the file with the most
   changed symbols, e.g. `16 changed symbols in 3 files — most in
   store/items.go (11)`. Computed from `graph.nodes` alone; no new data
   structures.

## Alternatives

- **Name-based DTO folding (`*Request` / `*Response` suffixes)**:
  precise for one Go idiom, useless elsewhere; a structural "childless
  type" criterion covers the same cases without encoding a naming
  convention.
- **Hiding leaf types entirely** (ADR 0009-style exclusion + flag):
  loses the "this method's contract changed" signal at the tree level
  and forces a flag; inlining keeps the information at lower cost.
- **Render-side grouping of methods under their interface** (post-pass
  matching `container` names): the receiver type name usually differs
  from the interface name in Go, so container matching is unreliable;
  method-spec references are exact and reuse the existing edge builder.
- **A dedicated `implements` edge kind in the graph model**: more
  faithful semantics, but ADR 0008 deliberately chose a name-based
  model; introducing typed edges for one rendering concern is not yet
  justified.
- **Interactive TUI instead of better static output**: does not fix the
  noise itself and forks the product into pipe and interactive modes;
  deferred until static output is as good as it can be.

## Consequences

- Markdown output shrinks and reads as "epicenter → callers": the
  summary line states the fan shape, interfaces group their methods,
  and data-carrier types no longer occupy tree lines.
- Another breaking change to default Markdown output on top of ADR
  0008/0009; acceptable pre-1.0.
- Interface→method linking is name-based and repo-global like every
  other edge, so an unrelated changed function that happens to share a
  method-spec name can be captured under an interface. Accepted as
  consistent with the ADR 0008 model's known trade-off.
- Adding method-spec names to `referenced_names` also feeds ADR 0003
  dependency resolution, so an interface's `Depends on:` may now list
  implementing methods — a desirable side effect for review.
- The inline `uses:` annotation is a new micro-format LLM consumers
  must parse; it is stable prose (`— uses: A, B`) and documented by the
  render tests.
