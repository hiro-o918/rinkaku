# 0034. Rename the "hotspot" vocabulary to "fan-in"

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0013 introduced the fan-in aggregation (`compute_hotspots`, the
`Hotspot` type, `Report.hotspots`, the Markdown `## Hotspots` section,
and the Mermaid `hotspot` node class) under the name "hotspot". The
TUI tree/dir badge surfacing the same aggregate went through two
amendments to that ADR: it started as the glyph `^N` (2026-07-13,
feat/file-size-warnings amendment), then became the text-labeled
`ref:N` badge in the same amendment pass, alongside `chg:N` and
(later, feat/label-contract-changes-badge) `api:N`.

User testing on the current `chg:N !1 ref:N` / `chg:N api:N ref:N`
tree row surfaced two independent problems with `ref:`, both reported
directly by the user driving the TUI:

1. **Collision with an unrelated keybinding.** `gr` ("jump to a
   caller", ADR 0022) is a completely different concept — a
   navigation command, not a count — but `ref:N` on a row and `gr` in
   the keymap read as related on first encounter, because both start
   with "ref". They are not related: `ref:N` is an aggregate over the
   fan-in graph, `gr` is a jump to one specific caller.
2. **"Hotspot" is an already-loaded term.** Tools like CodeScene use
   "hotspot" for a distinct metric (change frequency × complexity)
   that has nothing to do with fan-in. A reviewer arriving from that
   background reads rinkaku's `## Hotspots` section and expects a
   churn/complexity ranking, not a reference-count ranking. The ADR
   0013 Consequences section already flagged a related, milder
   collision (rinkaku's own informal "busiest file" hotspot wording in
   ADR 0012) as a naming risk to watch; this is the sharper version of
   that same risk landing in practice.

"Fan-in" has neither problem: it is a standard graph/software-metrics
term (in-degree — how many other things point at this one) with one
well-known meaning, it does not collide with any existing rinkaku
keybinding or wording, and the TUI's own detail pane (`crate::detail`)
already labels this exact number `fan-in: N` — the rename brings the
tree badge and the Markdown section in line with wording the codebase
already uses elsewhere, rather than inventing new terminology.

This ADR amends ADR 0013 (the section, type, and field it named) and
supersedes the two `ref:`-labeling amendments to ADR 0013 (the badge
label only — the underlying `Badges.fan_in` field name those
amendments explicitly left alone is, not coincidentally, already
correct).

## Decision

Rename every "hotspot" identifier and every user-facing "hotspot"
string to "fan-in", across all three output surfaces and the internal
data model that feeds them:

- **Rust identifiers** (`rinkaku-core`): `graph::Hotspot` →
  `graph::FanIn`, `graph::compute_hotspots` → `graph::compute_fan_ins`,
  `render::report::Report.hotspots` → `Report.fan_ins`. This is a full
  rename, not just the doc-facing labels — see the JSON discussion
  below for why identifiers and the wire format move together.
- **Markdown**: `## Hotspots` → `## High fan-in symbols`. Line format
  is unchanged (`- struct FooRequest (api/types.go) — used by 3:
  HandleFoo, HandleBar, SyncFoo`), only the heading text changes.
- **JSON**: the `hotspots` field renames to `fan_ins`, matching the
  Rust field rename above (`serde` has no `rename` override here — see
  Alternatives for why an override was rejected). This is a breaking
  change to the JSON schema.
- **Mermaid**: the `hotspot` node class (`classDef hotspot fill:...`)
  renames to `fan-in`. Mermaid class names accept a hyphen, so no
  further mangling is needed.
- **TUI tree/dir badge**: `ref:N` → `fan-in:N`. Number stays cyan
  (informational, not a warning like `api:`). `Badges.fan_in` (the
  struct field) is already named correctly per ADR 0013's amendment
  and needs no change — only the rendered label text moves.
- **TUI help overlay glossary** (`rinkaku-tui/src/help.rs`): the
  `"chg: / api: / ref:"` entry becomes `"chg: / api: / fan-in:"`, and
  its explanation is tightened to spell out the threshold in the term
  itself rather than leaving it implicit: "Tree row badges: changed
  symbols, contract changes (signature-changed or deleted, shown in
  yellow), and fan-in (symbols referenced by 2+ other changed symbols,
  shown as the fan-in badge's count)."
- **`docs/tui.md`**: the badge description and glyph-legend line that
  currently read "fan-in (hotspot aggregate)" drop the parenthetical —
  it is now redundant once the badge itself says `fan-in:N`.
- **Threshold constant**: promote the `>= 2` fan-in-eligibility literal
  in `compute_fan_ins` (`rinkaku-core/src/graph.rs`) to a named
  constant, `pub const HIGH_FAN_IN_THRESHOLD: usize = 2`, alongside a
  doc comment stating it is a judgment call (mirroring ADR 0013's own
  Consequences: "The fan-in >= 2 threshold is a judgment call, not
  derived from data") and that changing it is an ADR amendment, per
  the same convention `rinkaku-core/src/file_size.rs`'s thresholds
  already follow (CLAUDE.md's "File size discipline" section).

**Out of scope / left alone:** historical documents — past ADR bodies
(0013's own Decision/Consequences text, ADR 0014's "Hotspots entries"
line, ADR 0021's "hairball" reasoning, experiment round write-ups,
`CHANGELOG.md` entries) are a record of what was true at the time and
are not rewritten; only ADR 0013 gets a new Amendment note (below)
pointing forward to this ADR, following the same pattern its three
prior amendments already established. `README.md`'s and
`rinkaku/src/notes.rs`'s informal use of "hotspot" as an English
adjective (not the identifier/heading) is unaffected by this ADR's
scope, which targets the named concept, not the word in prose.

## Alternatives

- **Rename only the user-facing strings (Markdown heading, TUI badge,
  Mermaid class), keep `Hotspot`/`hotspots` as the internal
  identifiers and JSON field name**: considered, since JSON is a
  machine-consumer-facing contract and Rust identifiers are pure
  internals invisible to any consumer. Rejected: rinkaku is pre-1.0
  (workspace crates are all `0.x`, no tagged release has shipped a
  JSON consumer contract to break — same reasoning ADR 0013's
  amendments already used for the TUI badge relabeling), so there is
  no compatibility burden to preserve, and leaving the wire field
  named `hotspots` while every rendered surface says "fan-in" would
  produce exactly the internal inconsistency (one concept, two names,
  depending on which output format you read) this rename exists to
  eliminate. A `#[serde(rename = "hotspots")]` shim to decouple the
  Rust name from the wire name was also considered and rejected for
  the same reason: it would keep the inconsistency alive at the JSON
  boundary specifically, the one place a name mismatch is hardest to
  notice (no compiler to catch a stale reference, unlike Rust code).
- **Keep "Hotspots" as the Markdown heading, only fix the TUI badge and
  glossary**: addresses the `gr`/`ref:` collision (the sharper, more
  concretely reported problem) but leaves the CodeScene-collision risk
  ADR 0013 already flagged as a known risk unresolved in the surface
  most likely to be read by someone with that prior context (a
  Markdown report shared in a PR description, vs. an interactive TUI
  session). Rejected as solving only the more urgent half of the
  problem when both halves have the same fix.
- **Pick a different term than "fan-in" (e.g. "used-by", "referenced-
  by", "dependents")**: rejected because `crate::detail`'s detail pane
  already renders this exact number as `fan-in: N` (predating this
  ADR), so any other term would newly diverge from wording already
  shipped elsewhere in the TUI rather than converging on it.

## Consequences

- **Breaking change** to Markdown (heading text), JSON (`hotspots` →
  `fan_ins` field rename), Mermaid (`hotspot` → `fan-in` class name),
  and TUI (`ref:N` → `fan-in:N` badge, help glossary entry). Consistent
  with this project's stance (ADR 0013's own amendments; ADR 0008/0009
  /0012/0014's precedent) that pre-1.0 output-format changes do not
  need a compatibility path.
- Every internal identifier for this concept now agrees with every
  external string: `FanIn`/`compute_fan_ins`/`fan_ins` (Rust + JSON) and
  `fan-in`/`fan-in:N`/`High fan-in symbols` (Markdown/TUI/Mermaid) all
  read as the same concept, closing the gap ADR 0013's Consequences
  section predicted might need a follow-up pass.
- `HIGH_FAN_IN_THRESHOLD` becomes independently discoverable/greppable
  and documented as a judgment call, matching `file_size.rs`'s
  precedent — a future revisit (ADR 0013's own suggestion, "if
  dogfooding on real diffs shows fan-in == 1 entries carry useful
  signal") has one obvious place to look and one obvious place to
  change it.
- No change to the underlying aggregation logic, sort order, or tie-
  break rules `compute_hotspots` (now `compute_fan_ins`) implements —
  this ADR is a vocabulary and constant-extraction change only.
