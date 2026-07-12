# 0019. Entry-path pivot: re-rooting the change graph at a chosen path

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0008 auto-detects entry points: roots are the symbols nothing else
in the graph depends on, computed on the SCC condensation. That answers
"where does reading start" globally, but reviewers also ask a scoped
question: "what does this change (or this repository, since ADR 0017's
whole-repo mode) look like *from the api layer*?" — i.e. treat the
symbols under a chosen directory as the entry points and expand their
dependency trees outward, regardless of whether something outside that
directory calls into them. Whole-repo outlines sharpen the need: their
auto-detected root set is large and unfocused, and a path pivot is the
natural way to carve a viewpoint out of it. This is deliberately not
path *scoping* (analyzing only files under a path): the analysis stays
whole-graph; only the vantage point changes.

## Decision

Add a pure re-rooting function to `rinkaku-core`'s graph module: given
the existing `SymbolGraph` and a path prefix, the new roots are the
nodes whose file path is under the prefix and that no *other node under
the same prefix* depends on (ADR 0008's root rule applied within the
subset; edges arriving from outside the prefix do not disqualify a
root, because the pivot's whole point is to ignore the outside-in
direction). Dependency trees still expand outward through the full
graph. Expose it in two places: a `--entry <path>` CLI flag that
re-roots the report before Markdown/JSON rendering, and a TUI pivot —
`p` on a directory or file row switches the right pane to an
entry-tree view rooted at that row's path, rendered like the Markdown
entry trees and following the cursor until toggled off.

## Alternatives

- **Path scoping (filter the analysis to files under the path)**:
  simpler, but answers a different question — it hides the
  dependencies outside the path, which are exactly what a reviewer
  pivoting from a layer wants to see. Rejected; the memory of this
  distinction is the reason this ADR exists.
- **Making pivot a separate screen instead of a right-pane mode**:
  heavier UX for v1 and a bigger `App` state surface; a pane mode
  composes with the existing Detail/Diff toggle and scrolling for
  free. Revisit if the pane proves too small for real trees.
- **CLI-only (no TUI operation)**: cheaper, but the interactive
  "select a directory, look from here" gesture is the primary use
  case dogfooding asked for. Rejected.

## Consequences

- Whole-repo outlines become navigable by viewpoint: bare `rinkaku`,
  cursor on a layer directory, `p` — the layer's outward dependency
  shape, without re-running anything.
- A third right-pane mode joins Detail/Diff; the key surface grows by
  one (`p`), and the pane-mode state machine gets one more transition
  to keep scroll-reset rules consistent with (PR #54's blanket rule
  covers it automatically).
- Re-rooting is O(V+E) per invocation, computed on demand (CLI: once;
  TUI: on pivot toggle or cursor move while pivoted) — consistent
  with the crate's recompute-not-cache stance (ADR 0016).
- The root rule change is scoped: ADR 0008's global auto-detection
  stays the default everywhere; the pivot only applies under the flag
  or the TUI mode.
