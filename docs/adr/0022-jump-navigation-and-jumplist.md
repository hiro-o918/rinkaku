# 0022. Caller/callee jump navigation and a jumplist

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0020 gave the detail pane a `callers`/`callees` pivot (`DetailView`)
and the diff pane a hunk-jump binding (`]`/`[`), but reading a diff still
frequently raises a question the interaction model has no answer for:
"who calls this?" or "what does this call?" while looking at a symbol's
diff, not its aggregated detail. Today the only way to act on that
question is to press `d` to switch to the Detail pane, read the
`callers`/`callees` list as plain text, then manually scroll/search the
tree for a matching row — there is no way to jump the cursor there
directly, and no way back to the exact spot in the diff the reviewer was
reading before asking the question.

This is the "follow" step of a reviewer's reading loop, in the same spirit
as the map-assisted review workflow documented in
[experiment 0001](../experiments/0001-map-assisted-llm-review/README.md):
orient (map), read (diff pane), follow (jump to a related symbol), come
back. ADR 0020 built "orient" and "read"; this ADR builds "follow" and
"come back".

**Reference model**: neovim's `gd` (go to definition), `gr` (go to
references) and its jumplist (`Ctrl-o`/`Ctrl-i` to move backward/forward
through visited locations) — the same reference model ADR 0020 already
draws on for focus and hunk-jumping, so a reviewer fluent in neovim gets
this gesture for free too, and a reviewer who is not still gets one
consistent rule reinforced by the `?` overlay.

## Decision

**1. `gd`/`gr` jump to a callee/caller of the symbol under the cursor.**
`gd` ("go to definition") jumps toward what the selected symbol *calls*
(its callees); `gr` ("go to references") jumps toward what *calls* the
selected symbol (its callers) — matching neovim's own mnemonic split
(`gd` toward the thing defined, `gr` toward the things referencing it).
Candidates are read from the same edge set `crate::detail::build_detail`
already computes (`report.graph.edges`, deduped and self-edge-filtered)
via a small pure function extracted from that logic (`crate::detail::
symbol_mentions`) rather than a second, independent traversal — ADR
0020's own "one file, one responsibility" precedent applies at function
scope here too: the edge-walk-and-dedupe logic belongs in `detail.rs`
regardless of which caller (the Detail pane or this feature) needs the
result.

Only a symbol row is a valid jump source — `gd`/`gr` pressed on a
directory or file row sets a status-line message ("no symbol selected")
and does nothing else, since callers/callees are a per-symbol relation
with no defined meaning at the directory/file granularity (mirroring
`App::selected_diff_target`'s own `NodeKind::Dir => None` precedent).
Valid regardless of [`Focus`](../../rinkaku-tui/src/app.rs) (`Tree` or
`Right`) — the "thing being read" is the selected symbol either way, and
gating the gesture to one focus would only make the reviewer switch focus
first for no reason.

Candidate count decides what happens next:

- **Zero candidates**: a status-line message ("no callees" / "no
  callers") — a no-op otherwise, exactly like today's
  `App::selected_diff_target`'s `None` cases render a placeholder rather
  than erroring.
- **One candidate**: jump immediately (decision 3 below) — no popup for a
  single unambiguous target, since a selection UI for one option only
  adds a keystroke.
- **Multiple candidates**: open a selection popup listing every
  candidate (name + path), `j`/`k` to move the popup's own selection,
  `Enter` to jump to the highlighted one, `Esc` to cancel without
  jumping. Styled and composited the same way `?`'s help overlay already
  is (`ui::draw_help_overlay`'s `Clear` + centered bordered box) — a
  second, independent overlay concept was considered and rejected (see
  Alternatives) in favor of reusing that one.

**2. `g` is a minimal two-key prefix, not a new general chord engine.**
`App` gains one field, `pending_prefix: Option<PendingPrefix>`
(`PendingPrefix` today has exactly one variant, `G`), set when `g` is
pressed and cleared on the very next key regardless of what that key is.
If the next key is `d` or `r`, it resolves to `GotoDefinition`/
`GotoReferences`; any other key discards the pending prefix and falls
through to that key's own ordinary meaning (so `gg`, `gx`, or `g` followed
by an unrelated action never gets stuck waiting) — `crate::lib::
translate_key` owns this, mirroring how it already owns the `?`-overlay
short-circuit (ADR 0020) as the one place raw `crossterm` key sequences
become this crate's `InputKey`s, rather than teaching `App::handle_key`
about raw key codes. No timeout: a `g` press with no follow-up simply sits
pending until the next key arrives, which is fine for a single hard-coded
two-key sequence (unlike a general chord engine, where an un-resolving
prefix would need a timeout to avoid confusing the reviewer about why nothing
happened) — see Alternatives for why a general prefix-tree parser was
rejected in favor of this hard-coded one.

**3. A jump moves the tree cursor to the target symbol, expanding
collapsed ancestors, and keeps focus on [`Focus::Right`] showing that
symbol's Diff.** `crate::nav::Nav` gains
`move_cursor_to_symbol(symbol_id)`, a sibling to the existing
`move_cursor_to_path` (which deliberately excludes symbol rows — see that
method's own doc comment) rather than a generalization of it: pivoting
(`move_cursor_to_path`'s only caller) has no single-symbol-scoped
meaning, and jump navigation has no directory/file-scoped meaning, so the
two stay separate one-purpose functions instead of one function with a
kind-filtering parameter that would make both call sites less obviously
correct. `move_cursor_to_symbol` additionally expands every collapsed
ancestor directory/file on the path to the target — unlike
`move_cursor_to_path`'s "no-op if hidden under a collapsed ancestor"
contract (appropriate for `--entry`'s startup-time pivot, where a fresh
`App::new` is always fully expanded and a miss is genuinely a wrong
path), a mid-session jump target is very likely to be hidden by then, and
silently failing to jump because *some unrelated fold state* happens to
hide it would defeat the feature's own purpose. Focus stays `Right` (does
not force `Tree`) so the reviewer's reading flow is not interrupted —
the diff pane simply now shows the jumped-to symbol.

**4. A jumplist records where each jump came from, with `Ctrl-o`/
`Ctrl-i` to move back/forward through it**, mirroring vim's own jumplist
semantics: a new `Self::GotoDefinition`/`GotoReferences` jump pushes the
*pre-jump* location onto a back-stack (capped at 100 entries, oldest
dropped — a reviewing session realistically never needs more, and an
unbounded stack is an unnecessary unbounded-growth risk for a
long-running TUI session) and clears the forward-stack (any location the
reviewer had already jumped back past is now stale — vim's own jumplist
does the same: jumping to a new location from the middle of history
discards the abandoned future). `Ctrl-o` pops the back-stack, pushes the
*current* location onto the forward-stack, and moves there; `Ctrl-i` is
the mirror image. A recorded location is `(symbol_id, right_pane_scroll)`
— just enough to restore "what the reviewer was looking at", not a full
`App` snapshot, since jumping back should not also undo unrelated state
like the order mode or which overlay was open. Both are no-ops (with a
status message) when their respective stack is empty.

`crate::lib::translate_key` maps *both* a real `Ctrl-i` keypress
(`KeyCode::Char('i')` + `CONTROL`) and plain `KeyCode::Tab` to
`InputKey::JumpForward` — confirmed necessary via manual testing against a
real terminal (tmux), not assumed from documentation: Ctrl-I and Tab share
the same control code (0x09) at the terminal protocol level, so without
Kitty's keyboard-enhancement protocol (which this crate does not enable),
a genuine `Ctrl-i` press arrives as `KeyCode::Tab`, and the modifier-based
pattern alone silently never matches in practice. `Ctrl-o` needed no such
fallback (0x0F is not reused by another named key crossterm reports).

**Verification note**: this Tab/Ctrl-I mapping gap was only caught by
actually running the built binary in a real terminal and pressing the key
— unit tests alone (which construct `KeyEvent`s directly with the
modifier already set) could not have caught it, and did not. This is the
exact class of bug CLAUDE.md's "dynamic verification" review step exists
to catch.

**5. The popup's view-model is pure, built the same way `crate::help`'s
content is** — a plain struct (`JumpCandidate { id, name, path }` list)
computed once when the popup opens, not re-derived on every draw
tick, following ADR 0020 decision 5's caching discipline (`crate::run_app`
computes it once per handled key, hands it to `ui::draw`, which never
calls the computation itself).

## Alternatives

- **A general prefix-tree/chord parser in `translate_key` for arbitrary
  future `g`-prefixed bindings**: over-engineered for exactly one
  two-key sequence today: a real chord engine needs a timeout policy,
  ambiguity resolution between prefixes of different lengths, and a way
  to surface "waiting for next key" in the status line — none of which
  this feature needs yet. Rejected in favor of the minimal
  `pending_prefix: Option<PendingPrefix>` field; if a second `g`-prefixed
  binding is added later, revisit then rather than speculatively now.
- **A dedicated jump-target overlay type, separate from the `?` help
  overlay's rendering path**: would duplicate the `Clear` + centered
  bordered box + `Esc`-to-close plumbing `draw_help_overlay` already
  has. Rejected in favor of a second content variant drawn through the
  same compositing step (`ui::draw`'s "overlay draws last, on top of
  whatever screen was already rendered" structure already generalizes to
  more than one overlay kind without change).
- **Automatically disambiguating multiple candidates by proximity (same
  file first, same directory next) instead of a selection popup**: was
  considered so a common case ("call this same-file helper") never needs
  a popup at all. Rejected for v1: ADR 0020's own hunk-attribution
  precedent (multiple-symbol overlap resolved by *first* source-order
  match, not a fancier heuristic) favors the simplest rule that is still
  correct; proximity ranking can be layered on top of the popup's sort
  order later without changing this ADR's "0/1/many" branching.
- **Forcing focus back to `Tree` on jump** (so the reviewer sees the
  cursor land, tree-explorer style): rejected because the point of `gd`/
  `gr` is to keep reading the *content* of the jumped-to symbol, not to
  browse the tree — ADR 0020's own Enter-on-file/symbol-row behavior
  already established "drilling in moves focus right", and a jump is a
  more targeted version of the same drill-in, not a return to tree
  browsing.
- **Storing a full `App` snapshot per jumplist entry** (so `Ctrl-o` could
  restore literally everything, including order mode and open panes):
  rejected as both heavier (100 full `App` clones) and surprising —
  jumping back should undo *where you were reading*, not silently flip
  the order mode back too if the reviewer happened to toggle it in
  between. A `(symbol_id, right_pane_scroll)` pair is the minimal state
  that answers "what was I looking at".

## Consequences

- `App` gains two more pieces of state (`pending_prefix`, the jumplist's
  two stacks) and `InputKey` gains five more variants — a real complexity
  addition, accepted for the same reason ADR 0020 accepted `Focus`: a
  concrete, board-level dogfooding gap ("I can't act on 'who calls
  this?' without losing my place") costs more in practice than the added
  state-machine surface.
- `crate::detail::symbol_mentions`'s extraction is a pure refactor of
  already-tested logic (`build_detail`'s existing dedup/self-edge-filter
  tests continue to cover it indirectly); this feature's own tests cover
  the extracted function directly rather than only through
  `build_detail`.
- The jumplist is session-local (cleared on process exit, like every
  other piece of `App` state) — no persistence across TUI invocations is
  attempted or implied by this ADR.
- A future popup-driven feature (e.g. "jump to any hotspot") could reuse
  the same overlay-compositing path this ADR generalizes
  (`ui::draw`'s "draw last, on top" step) without another architectural
  decision.
