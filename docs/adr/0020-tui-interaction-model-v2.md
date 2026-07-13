# 0020. TUI interaction model v2: focus, default diff, selection-scoped content, and a help overlay

- Status: accepted
- Date: 2026-07-13

## Context

The TUI is unreleased (ADR 0015/0016), which means every keybinding
added so far (PR #49 navigation, #51 detail/diff toggle, #52 whole-repo
mode, #54 scrolling, #55 highlighting, #56 pivot) accreted independently,
feature by feature, with no unifying interaction model behind it. The
result, surfaced by dogfooding:

- Scrolling uses Shift+J/K, disjoint from the plain j/k the tree already
  uses for cursor movement — a reviewer has to remember a second,
  unrelated key pair for "the same kind of motion, but in the other
  pane".
- There is no notion of *which pane currently has the reviewer's
  attention*. j/k always moves the tree cursor, even while the reviewer
  is deep in a long diff and just wants to keep reading downward.
- The diff pane defaults to being hidden behind the detail pane
  (`RightPane::default()` is `Detail`), even though "what changed" is
  usually the first thing a reviewer wants to see, ahead of the
  aggregated used-by/callers view.
- The diff pane shows either a symbol's own line range or an entire
  file's hunks undifferentiated — a file selection dumps every hunk in
  source order with no indication of which hunk belongs to which symbol,
  and a contract change (ADR 0014's `SignatureChanged` +
  `previous_signature`) is buried inside the hunk text rather than
  called out up front the way the Detail pane already does
  (`SignatureView::Changed`, ADR 0015).
- There is no in-app help: every keybinding lives only in the README and
  the status line's single dense hint string
  (`ui::draw_status_line`), which cannot grow without becoming
  unreadable.
- The pivot pane already suffered one concrete bug from re-deriving
  per-frame content inside the draw loop instead of caching it on
  selection change (`lib.rs`'s `run_app`, the "per-frame pivot recompute
  bug" its own comments reference, fixed by computing
  `PivotSelection` once per handled key and threading it into
  `ui::draw`). Any new selection-derived content (the diff-shaping work
  below) must follow that same cache-on-change discipline from the
  start, not rediscover the bug independently.

This ADR fixes an interaction model addressing all of the above at once,
rather than patching each complaint as an isolated keybinding tweak —
the complaints share one root cause (no consistent mental model for
"what does a keypress mean right now") and a unified fix is cheaper than
five independent ones.

**Reference model**: neovim's window/focus idioms — a focus concept
where motion keys act on whichever pane has focus, `h` to move focus
left/back, `]c`/`[c` to jump between hunks in a diff, and `?` to open a
help overlay. Reviewers already fluent in a modal editor get these
gestures for free; reviewers who are not still get a single consistent
rule ("j/k always scrolls the thing with focus") instead of a
per-feature key.

## Decision

**1. Two-pane focus model.** Add `Focus { Tree, Right }` to `App`,
defaulting to `Tree`. Motion keys are interpreted relative to focus
rather than always targeting the tree:

- **Tree focus** (default): `j`/`k` move the tree cursor (the right pane
  keeps previewing the selection, unchanged from today). `Enter` on a
  directory row expands/collapses it (today's `Select` behavior,
  unchanged). `Enter` on a file/symbol row *additionally* moves focus to
  `Right` — drilling into a row's content is also a focus change, since
  that is the point of pressing Enter on a leaf. `Space` toggles
  expand/collapse on any row regardless of kind, same as today, and is
  scoped to `Focus::Tree` for the same reason `Enter`'s row-acting half
  is: a no-op while `Focus::Right`, rather than silently toggling
  whichever file/symbol row the cursor is parked behind the right pane
  currently being viewed.
- **Right focus**: `j`/`k` scroll the right pane by one line, reusing
  the existing `right_pane_scroll` state and its clamp-at-draw-time
  design (PR #54) rather than introducing a second scroll mechanism.
  `h` or `Esc` returns focus to `Tree`. While the right pane is showing
  the Diff view specifically, `]`/`[` (single keys, not neovim's `]c`/`[c`
  chord — Context's reference model is neovim's idiom, but rinkaku binds
  the plain bracket keys directly) jump the scroll offset to the start of
  the next/previous hunk (computed from the same shaped diff content this
  ADR introduces below, not by re-parsing).
- `d` (Detail/Diff toggle), `p` (pivot toggle), `s` (source drill-down),
  `o` (order toggle), `q` (quit) remain global and ignore focus
  entirely — they are mode/screen transitions, not motion, so folding
  them into the focus split would only add branches without adding
  clarity.

**2. Remove Shift+J/K as scroll bindings.** `InputKey::ScrollUp`/
`ScrollDown` are deleted; scrolling is now exactly "plain j/k while
`Focus::Right`". This is a strict simplification (one fewer key pair to
remember) enabled by focus doing the disambiguation Shift used to do.

**3. Diff becomes the default right pane.** `RightPane::default()`
changes from `Detail` to `Diff` — "what changed" is what a reviewer
wants first. The `--entry`/`with_entry_pivot` startup path is
unaffected: it already sets `right_pane` to `Pivot` explicitly after
`App::new`, which unconditionally overrides whatever `App::new` set as
the default, so `--entry --tui` continues to open straight into the
pivot tree regardless of this default's value.

**4. Diff pane selection-scope semantics**, replacing today's
undifferentiated file/symbol split:

- **Symbol selected**: unchanged from today — clip to that symbol's own
  line range (`hunks_for_range`), hiding hunks from sibling symbols in
  the same file.
- **File selected**: group the file's hunks under per-symbol section
  headers (the symbol's signature line as the header), in the order
  symbols appear in `report.files[..].symbols`. A hunk that intersects
  no symbol's line range at all (e.g. an import block, a module-level
  `use` change) is collected under one trailing "(module level)"
  section instead of being silently dropped or left floating without
  attribution. A hunk that intersects more than one symbol's range
  (possible when two symbols are adjacent with no gap) is attributed to
  the first symbol (source order) whose range it intersects — simplest
  rule that keeps every hunk under exactly one header rather than
  duplicating it.
- **Contract-change header**: when the selected symbol (or, for a file
  selection, each grouped symbol) has `previous_signature: Some(..)`,
  prefix its section with a 2-line `- <previous_signature>` /
  `+ <signature>` header, styled the same red/green as the Detail
  pane's own `SignatureView::Changed` rendering — before that symbol's
  grouped hunks. This puts the outline-level fact ("the contract
  changed, and to what") ahead of the implementation-level detail
  (the hunks), matching this ADR's own reference ordering: orient
  (what changed at the signature level) before reading the body.

**5. This diff-shaping computation is pure and cached, not
per-draw.** A new pure module builds the shaped diff content (grouped
sections, contract headers, hunk attribution) from `Report` + the
already-parsed `FileHunks` + the current selection — no `ratatui`
types, unit-testable the same way `crate::app`/`crate::tree` already
are. `crate::run_app` computes it once per handled key (mirroring the
existing `pivot_selection` cache, decision 6 below) and hands the
result into `ui::draw`; `ui::draw` itself must never call the shaping
function, for the exact reason `App::selected_pivot_view`'s own doc
comment already states: `terminal.draw` runs on every ~100ms idle poll
tick, not only on a key press, so computing selection-derived content
inside it re-does the work roughly ten times a second while idle. This
project has already paid for this mistake once (the pivot pane's
per-frame recompute bug, see Context) — the invariant here is: **any
content derived from `App`'s selection state is computed exactly once
per selection change, on the `run_app` side of the cache boundary,
never inside `ui::draw`.**

**6. `?` help overlay.** A centered overlay listing the current keymap
(varying by focus/screen) plus a short glossary (what "topological"
order means, what pivot does, what "cycle" refers to — the closing
back-edge of the dependency graph). `?` opens it; `?`, `Esc`, or `q`
close it. While the overlay is open, every key including `q` is
consumed by the overlay (closing it) rather than falling through to its
normal global meaning — an overlay that could be accidentally dismissed
by the app quitting underneath it would defeat its own purpose as a
safe, low-stakes "let me check the keys" action.

**7. Status line**: always show the current order mode
(`OrderMode::Topological`/`OrderMode::AlphaNumeric`, the exact terms
`crate::order` already uses — the line must not invent a different
label for the same concept). The key-hint segment switches based on
focus: a Tree-focused hint set (navigation-oriented) and a
Right-focused hint set (scroll/hunk-jump-oriented), plus a `?` mention
in both so the fuller reference is always one keypress away without
needing to fit everything on one line.

## Alternatives

- **Keep per-feature keys, add more as needed (status quo)**: this is
  exactly the pattern that produced the dogfooding complaints in
  Context — rejected as the thing this ADR exists to stop doing.
- **A single global scroll binding with no focus concept** (e.g. keep
  Shift+J/K, just document it better): cheaper, but does not fix "j/k
  always moves the tree cursor even when the reviewer's attention is in
  the right pane", which was the more specific complaint. Rejected.
- **Modal (full-screen replacement) help instead of a centered
  overlay**: a full-screen swap would lose the reviewer's place in the
  tree/pane layout entirely (rebuilding `Screen` state on return), where
  an overlay only needs to suspend key routing — cheaper state machine,
  and the reviewer's context stays visible dimmed behind it. Rejected
  in favor of the overlay.
- **Attributing a hunk touching multiple symbols to *every* overlapping
  symbol's section (duplicate it) instead of just the first**: would
  make "total lines shown" no longer match "total lines in the diff",
  actively misleading about change size for exactly the file-level view
  meant to summarize it. Rejected in favor of single (first-match)
  attribution.
- **Putting the diff-shaping logic in `ui.rs` directly**: `ui.rs` is
  already the crate's largest module (over 2000 lines) and is
  deliberately kept "thin adapter, pure view-model elsewhere" per its
  own doc comment; growing it further with grouping/attribution logic
  that has nothing to do with `ratatui` rendering would violate that
  split. Rejected in favor of a new sibling module.
- **Extending `diff_view.rs` in place instead of a new module**:
  `diff_view.rs` is scoped to parsing raw unified-diff text into
  `FileHunks`/`Hunk`/`DiffLine` — a different concern from grouping
  already-parsed hunks by symbol and prefixing contract headers, which
  reads `Report` and needs no diff-text parsing at all. Keeping them
  separate mirrors the crate's existing "one file, one responsibility"
  norm (`crate::tree` builds the tree, `crate::order` ranks it,
  `crate::detail` builds pane content — none of them absorb into a
  neighbor once each stays focused). This is CLAUDE.md's "split
  packages/modules when a responsibility grows large enough to warrant
  it" applied at module scope, not just crate scope; a >500-line
  `diff_view.rs` gaining a second, unrelated responsibility was judged
  past that point already.

## Consequences

- One consistent rule replaces five independent ones: motion keys act
  on whichever pane has focus, full stop. This is the ADR's primary
  discoverability win, made real for a first-time user via `?` rather
  than requiring the README to be read first.
- The interaction model has one more piece of state (`Focus`) and one
  more state-machine dimension to keep consistent (which keys are
  focus-aware vs. global) — a real complexity cost, accepted because
  the alternative (no focus, more per-feature keys) was shown by
  dogfooding to cost more in practice.
- Reviewers unfamiliar with neovim-style `h`/`]c`/`[c` idioms face a
  small learning curve; the `?` overlay's glossary is this ADR's
  mitigation, not a claim that the curve is zero.
- **No backward-compatibility concern**: the TUI has never shipped a
  release (ADR 0015/0016 introduced it, most recently extended by PR
  #56), so removing Shift+J/K and changing the default right pane are
  not breaking changes to any real user — this materially simplifies
  the decision, since there is no migration path to design, only a
  final shape to reach directly.
- The diff pane's file-selected view now costs more to compute (hunk
  attribution against every symbol's line range, rather than a flat
  hunk list) — bounded by file size and paid once per selection change
  under the caching rule in decision 5, not per draw, so this does not
  reintroduce the per-frame cost class the pivot pane's bug already
  demonstrated is a real risk in this codebase.
- A future `Resolver`-based hunk-to-symbol attribution (more precise
  than line-range intersection) could replace the shaping module's
  internals without changing this ADR's pane semantics — the module
  boundary (`Report` + `FileHunks` + selection in, shaped content out)
  is deliberately drawn to allow that later.

## Amendment (dogfooding, post-acceptance)

Decision 1 above states that a file/symbol row's `Enter` "additionally
moves focus to `Right`", without itself deciding what a symbol row's
`Enter` shows — that pre-dates this ADR (accreted incrementally, the
same way this ADR's own Context describes) and had a symbol row's `Enter`
open `Screen::Source` directly, reading the symbol's file from the
working tree, while a file row's `Enter` only switched focus. Dogfooding
surfaced this as an unpredictable failure ("enter だと source を出そうとし
てエラーになったりならなかったりする"): the working tree does not always
have the file in the state `Report` describes (a deleted file, a file not
yet checked out, a stale local branch), so the *same physical keypress*
sometimes worked and sometimes errored depending on row kind and
filesystem state the reviewer had no way to predict from the keymap
alone.

`Enter` on a file or symbol row now always performs the identical,
never-failing transition this ADR already describes for both row kinds:
switch the right pane to `RightPane::Diff` and move focus to `Right`.
Opening `Screen::Source` (the one operation in this whole interaction
model that touches the filesystem, and so is the only one that can fail)
stays behind the dedicated `s` key ([`InputKey::Source`]) exclusively —
consistent with this ADR's own frame that motion/pane keys are cheap,
predictable state transitions and `s` is the deliberate "go read the real
file" action. No other decision in this ADR changes: the diff-shaping
rules (decision 4), the pane-scoped `]`/`[` hunk jump (decision 1's Right
focus bullet), and the help overlay are all unaffected.

## Amendment 2 (map-assisted review, post-acceptance)

Two further scroll-semantics refinements, found by a map-assisted review
round rather than dogfooding, but belonging with the two amendments above
since both touch `right_pane_scroll`'s reset/clamp rules this ADR and
Amendment 1 already document:

- **Enter while already reading Diff must be a true no-op.** Amendment 1
  above unified `Enter` to always mean "switch to `RightPane::Diff` and
  move focus to `Right`" — but `App::handle_key`'s `(Screen::Entry, _,
  InputKey::Open)` arm matched regardless of `Focus`, so pressing `Enter`
  a second time mid-read (already `Focus::Right`, already `RightPane::Diff`)
  still fell through to the function's blanket "reset scroll to 0 unless
  `preserve_scroll`" rule and threw away the reviewer's reading position.
  `Focus::Right` now branches on the *current* `right_pane`: `Diff` already
  showing is a genuine no-op (scroll preserved, mirroring plain `j`/`k`
  scrolling's own `preserve_scroll` case), while `Detail`/`BlastRadius`
  showing is still a real pane switch to `Diff` and keeps the ordinary
  scroll-reset behavior, since that case *is* a content change.
- **`gd`/`gr` jumplist entries always recorded scroll 0.** `dispatch_non_source_key`
  (`crate/lib.rs`) must call `App::handle_key(GotoDefinition | GotoReferences)`
  before resolving candidates, purely so that call's unconditional
  `pending_prefix` clear runs (ADR 0022) — but that same call also hit the
  blanket scroll reset *before* `App::jump_to_symbol` read
  `right_pane_scroll` to save it into the jumplist entry being jumped
  *from*, so every entry's saved scroll was always 0 and `Ctrl-o` could
  never restore a real reading position. `GotoDefinition`/`GotoReferences`
  are now added to `preserve_scroll`'s exception list; `App::jump_to_symbol`
  (and `PopupConfirm`, which also calls it) still does its own explicit
  `right_pane_scroll = 0` reset for the *new* target once a jump actually
  happens, so this only protects the *old* position's snapshot, not the
  reset a real jump still performs.

A third, smaller point from the same review round was confirmed as an
already-intentional trade-off rather than a bug: `crate::run_app` folds
`ui::draw`'s clamped scroll back into `App` on *every* draw (Amendment to
`clamp_right_pane_scroll_after_draw`'s own doc comment, `lib.rs`), including
idle poll ticks, not only key presses — so shrinking the terminal
mid-read permanently clamps `App`'s own scroll offset down, and growing
the terminal back afterward does not restore the pre-shrink position.
Accepted as-is: `right_pane_scroll` is deliberately single-valued (no
separate "requested vs. actually-applied" pair to fall back to), and a
reviewer resizing mid-read and losing a few lines of scroll position is a
far rarer, lower-cost edge case than the overshoot-unwind bug this
fold-back exists to fix.
