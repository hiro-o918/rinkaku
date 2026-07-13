# 0027. Diff pane follows symbol selection: always render the whole file, auto-scroll to the selected symbol's section

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0020 decision 4 split the diff pane's selection-scope semantics
into two shapes: **symbol selected** clipped the pane to that symbol's
own hunks, hiding sibling symbols; **file selected** grouped every
hunk in the file under per-symbol section headers. This shipped and
matched the ADR's "orient before reading" framing at the individual
symbol level.

Dogfooding surfaced a specific complaint: moving the left-pane cursor
between symbols in the same file does not visibly "follow" on the
right — the diff pane's content swaps out entirely (from symbol A's
clipped hunks to symbol B's) and the scroll offset resets to 0. There
is nothing to *scroll* through, so `j`/`k` on `Focus::Right` is
useless while a symbol is selected, `]c`/`[c` only walks the one
symbol's own hunks, and the reader loses the surrounding-file context
they had while any file row was selected. It reads as "the diff pane
is not connected to the symbol I just moved to", even though the pane
is technically showing exactly that symbol's hunks — because the
motion produced no visible motion.

Two closely related properties fell out of the same clip-on-symbol
rule that the user noticed together:

- The reviewer cannot see, from a symbol row, the hunks of adjacent
  symbols in the same file. Those adjacent hunks are frequently the
  reason a change under the cursor was made — a caller a few symbols
  above, a helper a few symbols below — and today reaching them means
  moving the cursor off the symbol first, defeating "keep reading on
  this symbol".
- `]c`/`[c` (`InputKey::NextHunk`/`PrevHunk`) can only walk within one
  symbol's hunks while that symbol is selected, so a common "sweep
  every hunk in this file in order" gesture requires an extra
  intermediate step (pick the file row first, then bracket-walk).

These are all facets of the same underlying design choice — the diff
pane treats a symbol selection as a *filter* rather than as a
*focus point*. This ADR flips that.

## Decision

**1. The diff pane always renders the whole selected file.** Remove
`DiffPaneContent::Symbol(section)`; both file-row and symbol-row
selections now produce `DiffPaneContent::File(sections)` from
`crate::diff_shape::build_diff_pane_content`. The per-symbol section
grouping, module-level bucket, and contract headers established by
ADR 0020 decision 4 all stay exactly as they are for a file
selection — they just now also apply when a symbol row is selected.

**2. A symbol selection auto-scrolls to that symbol's section.**
When `App::selected_diff_target` resolves to a symbol row, the diff
pane's `right_pane_scroll` is set to the logical-line offset where
that symbol's section header starts within the shaped content, using
`crate::diff_shape::hunk_start_lines`' existing offset table. The
scroll offset is a normal `right_pane_scroll` value: `j`/`k`,
`Ctrl-d`/`Ctrl-u`, `gg`/`G`, and `]c`/`[c` all continue to work from
there, and the clamp-at-draw-time discipline
(`crate::ui::clamp_scroll`, folded back via
`clamp_right_pane_scroll_after_draw`) is unchanged.

**3. Section start, not first hunk start.** The auto-scroll target is
the section's *header* line (its signature line, and the contract
header above it if present) — not the first hunk's `@@` line. The
reviewer wants to see the section title / contract change first;
starting mid-header would hide exactly the outline-level fact ADR 0020
decision 4's contract header was added to surface.

**4. `run_app` owns the auto-scroll.** The offset is written to
`right_pane_scroll` in `crate::run_app`'s existing "recompute diff
pane content on selection change" step
(`should_recompute_diff_pane_content`), the same seam that already
rebuilds the shaped content and the highlight cache when the
selection changes. This keeps the auto-scroll rule in one place, next
to the state it derives from, and out of `App::handle_key` — which
has no access to `report` in the general case (that access is exactly
what forced `NextHunk`/`PrevHunk`'s own scroll computation to live in
`run_app` rather than `App`).

**5. Symbol-move preserves auto-scroll, not the manual scroll
offset.** `App::handle_key`'s blanket "reset `right_pane_scroll` to 0
on every key except `Up`/`Down` on `Focus::Right`" rule still runs;
the auto-scroll simply writes a fresh non-zero value right after,
based on the new selection. A reviewer who scrolled by hand within
one symbol's section and then moves the cursor to a sibling symbol
gets the sibling's section start — not their prior manual offset
translated onto the new section. This is the same "cursor motion
retargets the pane" principle every other right-pane content follows;
the alternative (preserve manual scroll across selection changes)
would have to invent a per-symbol scroll memory the rest of the pane
model does not have.

**6. `]c`/`[c` now walks every hunk in the file.** With the pane
always showing the whole file, `NextHunk`/`PrevHunk` naturally sweep
across every symbol's hunks in source order, from any row selection.
No change to `jump_scroll_target` itself — it already walks
`hunk_start_lines`, which now covers the whole file regardless of
selection kind.

**7. Removed-symbol / directory rows are unchanged.** A directory
row still shows the placeholder (`DiffTarget::None`), and a removed
symbol row still shows nothing (no line range to derive a scroll
target from). Only the *present* symbol row's semantics change.

## Alternatives

- **Keep the symbol-clip default and add a keybinding to expand to
  the whole file.** Rejected: this preserves the exact "moving the
  cursor does nothing visible" complaint that motivated the ADR, and
  adds a mode toggle on top. The default should already do the right
  thing.
- **Auto-scroll to the first *hunk* of the section instead of the
  section header.** Rejected: ADR 0020 decision 4 added the
  contract-change header specifically so the outline fact appears
  before the hunks; landing past it hides exactly what that header
  exists to show. Only applies when a contract header is present, but
  handling the two cases differently would put a per-section
  conditional into the auto-scroll rule for one line's saving.
- **Preserve `right_pane_scroll` across symbol moves (translate the
  offset onto the new section).** Rejected: introduces per-symbol
  scroll memory the rest of the right pane does not have, and would
  need a well-defined rule for "what does a scroll offset mean when
  moving from a 3-line symbol to a 30-line one" — a design surface
  bigger than the auto-scroll rule this ADR chose instead.
- **Cache one shaped `DiffPaneContent::File` per file and update only
  the scroll offset on symbol moves within the same file.** Rejected
  as a premature optimization: `build_diff_pane_content` on a file
  runs once per handled key on the currently selected file only,
  operates over already-parsed `FileHunks`, and is comfortably fast
  enough that the caching complexity is not justified. Can be revisited
  if profiling ever shows this recompute as a real cost.
- **Keep `DiffPaneContent::Symbol` around as dead code for potential
  future use.** Rejected: an unreferenced variant is a maintenance
  liability (every match on `DiffPaneContent` still has to handle it)
  for no current caller. Delete it; if a future feature needs a
  symbol-clipped view, add it back with a specific requirement to
  point at.
- **A dedicated "symbol focus" scroll indicator in the diff pane title
  (e.g. `Diff — fn foo`).** Considered as a way to make the auto-scroll
  visible even when the section is already fully on screen (no scroll
  motion). Deferred: not needed to fix the reported complaint, and
  ADR 0020 decision 4 already puts the symbol's signature at the top
  of its section as the header — that *is* the "you are focused
  here" indicator, right where the reviewer is reading. If dogfooding
  finds this insufficient a follow-up ADR can add a title suffix.

## Consequences

- The complaint the ADR was written to fix ("symbol 移動しても diff の
  scroll が連携しない") is fixed by construction: moving the left-pane
  cursor between symbols in the same file now visibly scrolls the
  right pane to the new symbol's section. Moving to a symbol in a
  different file rebuilds the shaped content for that file and lands
  the scroll at the new symbol's section start — same rule, different
  file.
- `DiffPaneContent::Symbol` and `build_symbol_content` are removed.
  Any test asserting a `DiffPaneContent::Symbol(_)` shape is rewritten
  to assert the corresponding `DiffPaneContent::File(vec![...])` shape
  (which was already the file-row expected value, so the
  post-migration expected values mostly already exist elsewhere in the
  same test module).
- `App::selected_diff_target` for a symbol row now returns
  `DiffTarget::File { path }` plus a separate "focus symbol id"
  channel (either as a second field on `DiffTarget`, or via a
  companion `selected_diff_focus` accessor — the implementation
  chooses the smaller change). Either way, the returned enum stays
  the shape `build_diff_pane_content` already accepts; the focus id
  is consumed by `run_app`'s auto-scroll step, not by the shaping
  function.
- Hunk-jump keys (`]c`/`[c`) become file-wide instead of symbol-scoped
  when a symbol is selected. This is a strict expansion — the same
  keys still work, they just walk more hunks. `should_apply_hunk_jump`
  is unchanged (still gated on `Focus::Right` + `RightPane::Diff`).
- `App::handle_key`'s existing scroll-reset-on-every-key rule needs no
  change: the reset still fires, then `run_app` re-derives the correct
  offset from the new selection. This keeps the reset rule uniform (no
  new "preserve scroll for symbol move" carve-out) and the auto-scroll
  rule in one place.
- No backward compatibility concern: the TUI has never shipped a
  release (ADR 0015/0016), so this replaces the ADR 0020 semantics
  in place rather than migrating between them.
- ADR 0020 decision 4's file-selection sub-rules (per-symbol section
  headers, module-level bucket for hunks intersecting no symbol,
  first-match attribution for overlapping symbols, contract header
  above signature-changed sections) all continue to apply — this ADR
  extends them to symbol selections rather than replacing them.
