# Using rinkaku with LLM reviewers

rinkaku's Markdown output can be handed to an LLM as a "map" before it
reads a diff. Ten rounds of a paired-arm experiment (see
[`experiments/0001-map-assisted-llm-review/`](experiments/0001-map-assisted-llm-review/README.md))
established what to expect and what not to:

- The map is a **complement, not a substitute** for a plain diff
  review. Across ten rounds, neither pass produced a superset of the
  other's findings — running both remains the defensible default.
- Its measurable value is **attention allocation** (routing toward
  integration seams, self-consistency defects, coverage boundaries),
  not token savings.
- **Dynamic verification** (building and executing the changed code
  against hostile / edge-case inputs) is the strongest single
  predictor of finding real behavioral defects. It has caught bugs
  the map cannot see by design: regressions in unchanged code, wrong
  values on data-flow wires, and anything outside language coverage.

The map does not verify anything. Treat it as an attention-allocation
aid.

## Recipe

1. **Generate the map from a trusted checkout** — a clean `main`
   build, never the branch under review, so a malicious or buggy diff
   can't tamper with the tool inspecting it:

   ```sh
   rinkaku --pr 123 --format md > map.md
   # or: rinkaku --base main --format md > map.md
   ```

2. **Paste `map.md` at the top of the reviewer's prompt**, followed
   by the actual diff, with instructions along these lines:

   ```
   Here is a structural map of this change (hotspots, contract markers,
   entry-point trees). Use it to decide where to read deeply first, but
   it is an attention-allocation aid, not a verifier: read the full
   implementation of anything it flags, and don't assume unflagged code
   is safe to skip. Then review the diff below.
   ```

3. **Run an independent pass without the map** alongside the
   map-assisted one; the two consistently surface different findings.

4. **Add a dynamic verification step** — build and actually execute
   the changed code, including failure-mode invocations (non-TTY
   stdin, empty input, missing files). Behavioral bugs don't show up
   on the signature surface the map draws from.
