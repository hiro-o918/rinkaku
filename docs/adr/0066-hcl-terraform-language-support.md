# 0066. HCL (Terraform) language support

Date: 2026-08-01

## Status

Accepted

## Context

Terraform repositories produce exactly the kind of PRs rinkaku exists
for: large, mechanically generated diffs (module upgrades, provider
bumps, environment fan-out) whose reviewable surface — which resources,
variables, and outputs changed shape — is much smaller than the diff.
ADR 0002 made language support additive: a `LanguageSupport` impl plus a
registry entry, with the extraction pipeline unchanged. HCL is the first
candidate language whose definitions are not functions/types but named
configuration blocks, which stresses four assumptions the v1 languages
share:

1. Definitions expose their name through a tree-sitter `name` field —
   HCL blocks are named by a block-type identifier plus quoted labels
   (`resource "aws_instance" "web"`).
2. A definition's name appears literally in its defining file —
   Terraform *references* (`var.region`) do not textually match their
   *definitions* (`variable "region"`), which breaks the documented
   invariant of `TagsResolver`'s aho-corasick prefilter.
3. One captured node = one symbol — a `locals` block defines one symbol
   per attribute, not one symbol for the block.
4. A language is identified by the path's final `.`-separated segment —
   the agreed scope (Terraform's `*.tftest.hcl` in, plain `.hcl`
   dialects out) is not expressible that way, since both end in `hcl`.

The grammar side is solved: crates.io `tree-sitter-hcl` 1.1.0
(Apache-2.0, tree-sitter-grammars org) binds via `tree-sitter-language
^0.1` — the same mechanism as the four existing grammar crates — and was
spike-verified to compile and parse against tree-sitter 0.26.

Discussed with the maintainer in issue #210; the decisions below record
the agreed answers to the four design points raised there.

## Decision

Add HCL (Terraform) as a built-in language using `tree-sitter-hcl`
1.1.0, registered for the path suffixes `.tf`, `.tofu`, and
`.tftest.hcl`.

1. **Registry routing becomes suffix-based** (issue #210 follow-up):
   `RegistryEntry` declares path *suffixes* instead of final extension
   segments, and `language_for_path` matches with `path.ends_with`.
   This keeps the registry declarative while letting an entry name a
   multi-segment convention (`.tftest.hcl`) without claiming every file
   whose final segment merely coincides (plain `.hcl`). Behavior for
   the existing entries is unchanged (`.rs`, `.go`, `.py`, `.ts`,
   `.tsx` match the same files); entries list more-specific suffixes
   first as a convention. Plain `.hcl` dialects (Packer, Nomad) stay
   out of v1 — Terraform-flavored reference prefixes on non-Terraform
   dialects would mislead more than help; widening later is additive.

2. **Definitions are top-level blocks**, captured as
   `(config_file (body (block) @definition))`. Nested blocks (`tags`,
   `dynamic`, provisioners) are never captured. The recognized block
   types and their symbol names use **Terraform reference syntax**, so
   a definition's name equals the string other files use to reference
   it and the existing name-keyed `TagsResolver` and `collect_edges`
   work unchanged:

   | Block | Symbol name |
   |---|---|
   | `resource "T" "N"` | `T.N` |
   | `data "T" "N"` | `data.T.N` |
   | `module "N"` | `module.N` |
   | `variable "N"` | `var.N` |
   | `output "N"` | `output.N` |
   | `provider "P"` | `provider.P` |
   | `locals { a = … }` | `local.a` (one symbol per attribute) |

   `terraform`, `moved`, `import`, `check`, and unrecognized block
   types are not reported. A `locals` block expands to one symbol per
   attribute — per-name fan-in is the point of name-keyed matching —
   via a one-node-to-many `build_symbols` dispatcher in `extract`
   (capturing attributes directly in the query would poison the
   narrowest-enclosing-definition suppression for resource bodies).

3. **Signatures follow the contract/implementation split** the other
   languages already encode: `variable` and `output` blocks keep their
   whole text (type/default/description/value *are* the contract, so
   editing them classifies as `signature_changed`); `resource`, `data`,
   `module`, and `provider` blocks keep only their header (the body is
   implementation, so argument edits classify as `body_only`); a
   `locals` attribute keeps its whole text.

4. **References are collected by a code walk, not the reference
   query**: HCL references are traversals
   (`(variable_expr (identifier)) (get_attr …)*`), normalized to the
   same reference syntax as definition names — `var.A`, `local.A`,
   `module.A`, `data.A.B`, and `T.A` for resource references; the
   meta-roots `each`, `count`, `self`, `path`, and `terraform` are
   dropped. The walk lives beside the existing Rust-only walks (ADR
   0063/0064) and is inert for other grammars (`variable_expr` exists
   only in the HCL grammar). The reference query itself captures only
   `function_call` names (HCL built-ins simply fail to resolve, the
   same non-resolving story as Go's built-in types). Per-language
   extraction logic stays in `extract`'s flat match arms and walks —
   promoting extraction hooks into `LanguageSupport` is deliberately
   deferred to a follow-up ADR once HCL's concrete shape exists
   (issue #210, design point 1).

5. **The prefilter invariant is restored by pattern expansion**:
   `TagsResolver::new` adds each dot-separated component of a dotted
   reference name to the aho-corasick pattern set, so
   `variable "region"`'s file passes the prefilter for the reference
   `var.region` via its `region` component. Every component is added,
   including single-character ones (`variable "x"` must stay findable
   for `var.x`) — a short pattern passes more files through the
   prefilter, which costs parsing time, never recall.

6. **`.terraform.lock.hcl` is treated as generated by path**, mirroring
   GitHub linguist's own path rule for it. Under this ADR's suffix
   scope no `LanguageSupport` ever claims the lock file (it matches
   none of `.tf`/`.tofu`/`.tftest.hcl`), so it could never produce
   symbols; the path rule exists to label it `generated` rather than
   `unsupported_language` in `skipped` on the very common provider-bump
   PR, and to future-proof any later widening to plain `.hcl`. The
   check applies only in `analyze_diff`'s skip classification and
   carries its own explicit `--include-generated` guard — unlike the
   `.gitattributes` set, which the caller empties when the flag is
   set, a path predicate has no caller-side off switch.

7. **`*.tftest.hcl` files are test files** (`is_test_path`), matching
   Terraform's native test convention. Node-level `is_test_definition`
   stays the default `false`.

8. **SymbolKind gains one generic `Block` variant** (issue #210, design
   point 2), rendered as `block`; the name already carries the
   Terraform-specific semantics (`var.region`, `aws_instance.web`),
   the enum's language-neutral naming rule stays intact, and finer
   kinds later would be additive JSON output changes.

9. **No TUI syntax highlighting for HCL in v1**: tree-sitter-hcl
   exports no highlights query; `highlight.rs` falls back to plain text
   gracefully. Vendoring `queries/highlights.scm` (Apache-2.0 notice
   required) is a possible follow-up.

### Landing strategy

Per the maintainer's request in issue #210, the language-neutral
shared-code changes land as small preparatory PRs so the HCL PR itself
stays close to purely additive:

1. This ADR.
2. Suffix-based registry routing (decision 1).
3. `symbol_kind` takes source bytes (block-type dispatch needs
   identifier text).
4. `build_symbol` → `build_symbols` one-node-to-many dispatcher
   (decision 2's locals expansion, landed behavior-neutral).
5. `.terraform.lock.hcl` generated-path check (decision 6).
6. HCL itself: grammar dependency, `language/hcl.rs`, extraction
   arms, reference walk, prefilter expansion, `SymbolKind::Block`,
   tests, docs.

Mermaid needs no preparation: node ids are already mapped to a
sequential mermaid-safe `n{i}` space precisely because `NodeId`s
contain `/`, `:`, `@`, and `.` — dotted HCL names ride the existing
mechanism, pinned by a rendering test in PR 6.

## Alternatives considered

- **hcl-rs / non-tree-sitter parsing** — rejected: ADR 0002's whole
  premise is one extraction pipeline over tree-sitter grammars.
- **Naming definitions by bare label** (`region` instead of
  `var.region`) — rejected: collides across block types (`variable
  "web"` vs `resource "aws_instance" "web"`), and reference text would
  need de-prefixing anyway, losing the exact-match property with
  `collect_edges`/`TagsResolver`.
- **Registering plain `.hcl` and `.tf.json`** — rejected for v1: plain
  `.hcl` dialects would get misleading Terraform-flavored prefixes;
  `.tf.json` is JSON syntax with a different grammar.
- **A path-filter callback in front of an extension-keyed `hcl` entry**
  (instead of suffix matching) — rejected: pushes routing logic into
  code where the registry is otherwise declarative data; suffix
  matching expresses the same scope as data.
- **Per-language extraction trait now** — deferred, not rejected: HCL
  is the first concrete second use case, but the right shape for the
  hooks is only knowable once HCL's extraction logic exists; a
  follow-up ADR will revisit (issue #210, design point 1).
- **`#eq?` query predicates for block-type filtering** — rejected: the
  Rust binding leaves text predicates to the caller, and rinkaku's raw
  `QueryCursor` iteration deliberately does not evaluate them; the
  established pattern is per-language dispatch in `extract` code.

## Consequences

- Terraform PRs get symbol-level condensation; multi-environment repos
  benefit from the existing path-proximity ranking for same-named
  resources.
- JSON output gains one new `kind` value (`Block`); Markdown/TUI gain
  the `block` label. Additive, not breaking. In the Markdown change
  graph, `Block` symbols fold like the other non-function kinds
  (`is_foldable`): a `var.region` repeated under several resources
  reads as a repeated data shape, "(see above)" on repeats.
- `language_for_path` routes by suffix rather than final segment: a
  file literally named `rs`/`go`/`py`/`ts` (no dot) no longer routes to
  a language — an intentional fix that falls out of the change.
- `extract/mod.rs`'s `symbol_kind` takes the source bytes, and its
  "node kinds are unique across grammars" doc invariant is rewritten —
  `block` exists in other grammars but is never captured by their
  definition queries.
- The prefilter's documented "a definition's name always appears
  literally in its own declaration" invariant is amended to cover
  dotted-name component expansion.
- The entry-point ordering (ADR 0008) degrades meaningfully for
  Terraform: roots are resources/outputs nothing references;
  `var.*`/`local.*` sink to the bottom as high-fan-in foundations. The
  graph remains an approximation (no `count`/`for_each` expansion, no
  provider edges — ADR 0003's accepted trade-off).
- Provider-defined types (`aws_instance`) never resolve as
  dependencies — consistent with built-in types in other languages.
- Cross-module output references (`module.vpc.subnet_id`) resolve to
  the `module.vpc` block, not the inner `output` — accepted under the
  1-hop philosophy.
- OpenTofu's `*.tofutest.hcl` test convention is not registered in v1;
  adding it later is a one-line suffix addition.
