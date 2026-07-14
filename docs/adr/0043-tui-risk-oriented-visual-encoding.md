# 0043. Risk-oriented visual encoding for the TUI entry tree

- Status: accepted
- Date: 2026-07-14
- Related: [ADR 0035](0035-tests-sorted-last-in-entry-tree.md) (whole-test-file
  section, per-symbol `test` badge this ADR removes),
  [ADR 0039](0039-mermaid-visual-encoding-revision.md) (the Mermaid
  counterpart of this ADR's `!` marker and fold),
  [ADR 0042](0042-exclude-test-referrers-from-fan-in.md) (the fan-in count
  this ADR's `!` marker reads)

## Context

`docs/tui.md`'s reading protocol — read bright/marked rows, skim dimmed
ones — has so far lived only as README prose the reviewer must already
know before looking at the screen. Two gaps in the entry tree work
against that protocol once a real diff is loaded:

1. **A mixed file's test symbols compete with production symbols for
   attention.** ADR 0035 already moved whole test files to a trailing
   "Tests" section, but a file mixing production and test code (a Rust
   file with a `#[cfg(test)] mod tests` block is the common case in
   this very repository) keeps its test symbols as ordinary `Symbol`
   children, one row each, with a trailing `test` badge (ADR 0035's
   change 3). A file with a dozen `#[test]` functions floods the tree
   with a dozen individually-badged rows — exactly the "drowns out the
   implementation symbols" problem ADR 0009 already solved once for the
   default output, now recurring one level down inside the TUI tree.
2. **Fan-in and contract-change are both visible per-row, but their
   co-occurrence is not.** `chg:`/`api:`/`fan-in:` badges (ADR
   0013/0034) already surface each count independently, and a reviewer
   is meant to weight "signature changed" and "widely referenced"
   together as the highest-risk combination — but nothing on the row
   itself says "these two co-occur here," so noticing the combination
   requires reading both numbers and doing the AND in your head, for
   every row, every time.

[ADR 0042](0042-exclude-test-referrers-from-fan-in.md) lands alongside
this ADR and changes what the fan-in count itself means (production
referrers only); this ADR is the TUI-side consequence of that count
now being a trustworthy risk signal worth encoding directly on the row.

## Decision

Three changes to the entry tree's rendering, in `rinkaku-tui`:

**1. Fold a mixed file's test symbols into one collapsed `TestGroup`
child.** `tree::NodeKind` gains a `TestGroup { count: usize }` variant,
built only for the `production` `TreeBuilder` (a whole test file is
already routed into ADR 0035's "Tests" section and stays flat — folding
it again one level deeper would be redundant nesting with nothing left
to distinguish it from). `TreeBuilder::group_test_symbols` (`bool`)
switches this per builder instance. The group node renders as a single
`N tests`/`1 test` row, dimmed (`Color::DarkGray`), and starts collapsed
by default (`Nav::new_collapsing_test_groups`, seeding the initial
`collapsed` set with every `TestGroup` path) — a reviewer sees the count
and chooses to expand, rather than scrolling past every test row to
reach the next production file. Expanding shows the symbols with dimmed
names and no per-symbol badge; the group label already conveys "this is
test code," so the per-symbol `test` badge ADR 0035 introduced is
removed as redundant once its only remaining home (a mixed file) has a
group label saying the same thing.

**2. A `!` marker (bold red) flags contract-change/fan-in co-occurrence.**
Prefixes a `Dir`/`File` row when its aggregated `Badges` show both
`contract_changes > 0` and `fan_in >= HIGH_FAN_IN_THRESHOLD` in the same
subtree, and a `Symbol` row when it is itself `SignatureChanged` and its
own fan-in (the test-referrer-excluded count, ADR 0042) clears the same
threshold. A non-risky row's layout is untouched: the marker and its
trailing space are only pushed when risky, no reserved gutter column
holding a blank placeholder on every other row.

**3. Body-only/unclassified symbol names dim to `DarkGray`.** Previously
only a removed symbol's name carried its own style (dimmed +
crossed-out); a body-only or unclassified symbol's name rendered in the
default style, i.e. visually equal to an added or signature-changed
symbol's name despite carrying materially less review weight. Dimming
(without strikethrough, since the symbol still exists) makes the "read
bright, skim dim" protocol legible on the name span itself instead of
requiring the reviewer to cross-reference the leading marker character.

The help overlay glossary (`rinkaku-tui/src/help.rs`) gains entries for
`!` and `N tests`, and the existing `chg: / api: / fan-in:` entry is
reworded to note that fan-in counts production referrers only,
reflecting ADR 0042.

## Alternatives

- **A reserved gutter column for the risk marker (always-present, blank
  when not risky)**: keeps every row's label starting at a fixed
  column, which helps horizontal scanning. Rejected: the entry tree
  already has no fixed-width columns for its other conditional spans
  (badges, the `test`/`[test]` markers, the classification marker) —
  every one of them is push-if-present, consistent with `row_view.rs`'s
  existing convention — and a gutter reserved for a marker that fires
  rarely (contract-change AND high-fan-in is meant to be an unusual,
  attention-worthy combination) would visually widen every ordinary row
  for a signal most rows never carry.
- **Keep the per-symbol `test` badge alongside the new `TestGroup`
  label**: considered, since the badge and the group label both say
  "this is test code" and could in principle coexist. Rejected as
  redundant — the group node's very existence and its `N tests` label
  already convey membership for every child underneath it; repeating
  the same fact on each child row adds visual noise (an extra span per
  row) without adding information, the opposite of the fold's own goal
  of quieting the mixed-file test rows down.
- **A directory-level `!` aggregate distinct from the file/symbol
  marker (e.g. counting how many risky subtrees exist)**: rejected for
  now — a `Dir` row already inherits the `!` marker via the same
  `Badges` aggregation logic used for `chg:`/`api:`/`fan-in:` roll-ups,
  so a separate counted variant would duplicate a signal the boolean
  marker already surfaces at every ancestor level; revisit only if
  dogfooding shows a reviewer wants to know *how many* risky subtrees
  sit under a directory, not just whether one exists.
- **Remove test edges/nodes from the tree entirely instead of folding**:
  rejected for the same reason ADR 0042 rejected dropping test edges
  from `SymbolGraph`: "tests were touched" is itself a signal a
  reviewer wants (did this change come with tests?), which ADR 0009
  already established as worth preserving even while de-emphasizing the
  content. Folding preserves the count and lets a reviewer opt into the
  detail; removing would discard the "were there tests" fact along with
  the noise.

## Consequences

- `tree::TreeNode::kind` gains a `TestGroup` variant, so every existing
  exhaustive `match` over `NodeKind` (`row_view.rs`, `order/sort.rs`,
  `nav.rs`) needs a new arm — compile-time-enforced, same category of
  churn ADR 0035's `Section` variant already caused.
- Breaking change to the TUI's row rendering only (no Markdown/JSON/
  Mermaid output is touched by this ADR); consistent with this
  project's pre-1.0 stance on TUI presentation changes (ADR 0013's
  amendments, ADR 0034).
- The per-symbol `test` badge (`row_view::symbol_test_badge_span`,
  introduced by ADR 0035) is removed; the whole-file `[test] (N
  symbols)` badge for a file under the "Tests" section is unchanged —
  this ADR's fold only affects mixed files, not whole test files.
- The `!` marker's threshold is entirely derived from
  `HIGH_FAN_IN_THRESHOLD` (ADR 0034) and `contract_changes`/
  `Classification::SignatureChanged` (ADR 0014); it introduces no new
  constant of its own, so a future revisit of the fan-in threshold
  automatically retunes the marker without a separate change here.
- `Nav::new_collapsing_test_groups` is additive alongside `Nav::new`;
  callers that construct a bare `Nav` (e.g. some existing tests) keep
  every `TestGroup` expanded by default, which is intentional — only
  the TUI's actual startup path is expected to call the collapsing
  constructor.
