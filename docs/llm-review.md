# Using rinkaku with LLM reviewers

rinkaku's output works well as a "map" handed to an LLM before it reads a
diff: the hotspots, contract markers (added/removed/signature-changed),
and entry-point trees let the reviewer allocate deep-reading attention
instead of reconstructing the change's structure itself. For the
underlying `--format md`/`--format json` flags and what their output
looks like, see [CLI usage and output format](cli-usage.md).

1. Generate the map from a **trusted checkout** — a clean `main` build,
   never the branch under review, so a malicious or buggy diff can't tamper
   with the tool inspecting it:

   ```sh
   rinkaku --pr 123 --format md > map.md
   # or: rinkaku --base main --format md > map.md
   ```

2. Paste `map.md` at the top of the reviewer's prompt, followed by the
   actual diff, with instructions along these lines:

   ```
   Here is a structural map of this change (hotspots, contract markers,
   entry-point trees). Use it to decide where to read deeply first, but
   it is an attention-allocation aid, not a verifier: read the full
   implementation of anything it flags, and don't assume unflagged code
   is safe to skip. Then review the diff below.
   ```

3. Run an **independent pass without the map** alongside the map-assisted
   one. Across repeated trials, the two passes consistently surface
   different findings rather than one being a superset of the other — see
   [Experiment 0001: map-assisted LLM review](experiments/0001-map-assisted-llm-review/README.md).

4. Whichever pass(es) you run, always add a **dynamic verification**
   step: build and actually execute the changed code, including
   failure-mode invocations (non-TTY stdin, empty input, missing files).
   Behavioral bugs don't show up on the signature surface the map draws
   from, and the experiment's own rounds found real defects (a non-TTY
   panic) only by running the binary.
