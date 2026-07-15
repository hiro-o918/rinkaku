# 0057. TUI source view search, and splitting the help overlay's Global group by screen

- Status: Accepted
- Date: 2026-07-16

## Context

`Screen::Source` (ADR 0026) already gives a reviewer line-by-line and
half-page motion through a symbol's file, but no way to jump to a
specific place by content — finding a call site or a string literal
several hundred lines away still means scrolling past everything in
between. A vim-like `/` search is the obvious gap: every other motion
primitive in this screen already mirrors a vim idiom (`j`/`k`,
`Ctrl-d`/`Ctrl-u`, `gg`/`G`), so `/`, `n`, `N` complete the set rather
than introducing an unrelated interaction style.

Extending search to the Entry screen's Diff pane is out of scope here —
that pane's content is shaped/grouped per symbol (`crate::diff_shape`)
rather than a flat line list, so "what a match position means" would
need its own design (does a match jump reset the diff-focus auto-scroll?
does it search across the whole file or only the selected symbol's
section?). None of those questions are settled by this ADR; Source view
alone has a flat `Vec<String>` of lines, an already-simple case worth
shipping on its own.

Separately, PR #177 (`?` help overlay per-screen filtering) shipped
`applicable_help_groups`, which already splits the overlay into
`TreeFocus`/`RightFocus`/`SourceView`/`Review`/`Global` groups filtered
by the reviewer's current screen/focus — except `Global` is not actually
global: on `Screen::Source`, `App::handle_key`'s catch-all arm
(`(Screen::Source { .. }, _, _) => {}`) swallows `d`/`r`/`o`/`s`/
`ctrl-o`/`ctrl-i` as no-ops, while `v`/`w`/`u`/`gd`/`gr`/`?`/`q` do work
there (`ToggleSplitView` has its own dedicated Source-screen arm; `w`/`u`
are dispatched before `App::handle_key`'s Source catch-all even runs,
per `crate::event_loop::run_app`'s inline special-casing; `gd`/`gr`
degrade to a "not applicable" status message rather than a true no-op,
since `resolve_goto` still runs, but produce no navigation effect on this
screen either way). PR #177's own body recorded this as a known,
deliberately deferred limitation. This ADR resolves it as a byproduct of
adding the new Source-screen bindings, since both changes touch
`help.rs`'s group table.

## Decision

### Search (Source view only)

**1. `/` starts Source-screen-only search mode.** Pressing `/` while
`Screen::Source` is open enters composing mode; `/` is otherwise unbound
on this screen today, so no existing gesture is displaced. `/` is *not*
translated to a search action anywhere else (`Screen::Entry`, the help
overlay, any popup) — `crate::input_translate::translate_key` only
recognizes it while `on_source_screen` and no higher-priority modal
(review overlay, jump popup, update prompt) is open, mirroring how
`InputKey::Source`'s own `s` binding is likewise screen-scoped.

**2. Composing-mode key handling**, modeled directly on the review
overlay's `Compose` mode (`ReviewMode::Compose`, ADR 0048) rather than a
new pattern:

- Printable characters append to the query buffer.
- Backspace removes the last character.
- Enter confirms the search: computes matches, jumps to the first match
  at or after the current `scroll_top`, and leaves composing mode.
- Esc cancels: discards the in-progress buffer and returns to plain
  reading with no active search (any *previous* confirmed search's
  matches/highlighting are also cleared — cancel means "stop searching
  altogether", not "revert to the last confirmed query"). Esc has this
  same clearing effect even outside composing mode, whenever a confirmed
  search is still active (matching vim's own `/`-then-Esc convention of
  dismissing the highlight): `crate::input_translate::translate_key`
  checks `app.search().query().is_some()` and emits `SearchCancel` before
  falling through to the screen's ordinary `Back` meaning, so a reviewer
  backing out of a search does not also lose their place by leaving the
  screen in the same keypress. Leaving Source by *any* path — `Esc` above,
  or `q`/`InputKey::Back` — clears search the same way: `App::handle_key`'s
  `(Screen::Source, _, InputKey::Back)` arm cancels `search`
  unconditionally, so re-entering Source on a different symbol never shows
  a stale query/match count against unrelated content.
- Enter confirming against a Source screen whose file failed to load (no
  `Ok` `source_content` — a deleted file, a permission error) cancels the
  search rather than leaving it composing: neither `/` nor Enter is gated
  on load success, so this is reachable, not just defensive, and leaving
  Enter a no-op would trap the reviewer in the minibuffer with only Esc as
  a way out.

**3. The status line doubles as a minibuffer.** While composing,
`crate::ui::status::draw_status_line` renders `/` followed by the
buffer's current contents in place of the ordinary help-hint text, the
same "status line is the one line available for transient input/output"
role the line already plays for `App::status`'s transient messages.
No separate popup/overlay is introduced for this — a one-line buffer
does not need one, and a popup would additionally have to solve focus
routing this composing mode already gets by piggybacking on
`ReviewMode::Compose`'s existing "one key space, one modal priority
slot" precedent (`App::handle_key`'s own doc comment on why the review
overlay is checked first).

**4. `n` / `N` jump to next/previous match**, wrap-around, vim
convention (`n` repeats the last search forward, `N` repeats it
backward). Both are Source-screen-only bindings — `crate::input_translate::translate_key`
only emits `InputKey::SearchNext`/`SearchPrev` for `n`/`N` while
`on_source_screen`, so they do not collide with the Entry screen's
existing `n` (`NoteCompose`, ADR 0048) / `N` (`NotesList`) review-note
bindings, which stay exactly as they are.

`N` (uppercase) is acceptable here under this project's "no arbitrary
case-insensitive keybindings" convention (uppercase is reserved for a
distinct operation from its lowercase counterpart, not a case-insensitive
alias — see PR #177, which dropped the case-insensitive uppercase
aliases this convention now forbids) because `N` means "jump to the
*previous* match", a genuinely separate operation from `n`'s "jump to
the *next* match" — not a case-insensitive spelling of the same action.
This is the same justification ADR 0026 already gives for `gg` vs. `G`
(top vs. bottom are different destinations, not the same one typed two
ways).

When no search is active (never confirmed a query this screen-visit, or
the last confirmed query had zero matches), `n`/`N` are no-ops — there is
nothing to jump between.

**5. Match semantics: literal substring search with smartcase.** Not
regex — v1 scope is "find this text", and a regex engine is a
disproportionate dependency for that. Smartcase (the vim/ripgrep
convention already familiar to this feature's target audience): a query
that is entirely lowercase searches case-insensitively; a query
containing any uppercase character searches case-sensitively. This
needs no configuration surface (no separate "case sensitive" toggle) and
matches what a vim-literate reviewer already expects from `/`.

**6. Matched-line highlighting reuses `source_screen.rs`'s existing
diff-overlay background-color-blend approach** — the same `Option<Color>`
background-tint composition `unchanged_line`/`SOURCE_HIGHLIGHT_BG`/
`ADDED_BG` already layer (ADR 0018's "token foreground + line-level
background tint", extended by ADR 0046's diff overlay). A new
`SEARCH_MATCH_BG`/`SEARCH_CURRENT_MATCH_BG` pair of tint constants is
added to that same layering, not a new rendering path — search
highlighting composes with the existing symbol-range tint and diff
overlay exactly the way those two already compose with each other,
using the same "more specific signal wins" precedent
(`unchanged_line`'s own doc comment: `diff_bg.or(is_highlighted...)`)
extended one level further: `diff_bg.or(match_bg).or(range_bg)`. The
*current* match (the one `n`/`N`/the confirming Enter just navigated to)
uses a visually distinguishable tint from other matches on screen
(`SEARCH_CURRENT_MATCH_BG`, brighter/more saturated than
`SEARCH_MATCH_BG`) — a reviewer scanning a screen with several matches
needs to see which one the cursor actually landed on without counting.

**7. Status line shows `N/M` after confirming a search.** Once a query is
confirmed (Enter, or a `n`/`N` jump), the status line's help-hint segment
is replaced by `/query — N/M` (`N` = current match's 1-based position,
`M` = total match count) until the reviewer presses a key that leaves
search context (Esc, `Back`, or starting a new `/` search). Zero matches
render as `/query — no matches` — `N/M` has no sensible value when
`M == 0`, so this is a distinct display case, not `0/0`.

### Help overlay: split Global by screen reachability

**8. `HelpGroup::Global` splits into two groups**, along the exact fault
line PR #177's own "Known limitation" section already identified:

- `HelpGroup::Global` (kept name, narrowed content): bindings valid on
  *every* screen — `v`, `w`, `u`, `gd`, `gr`, `?`, `q`/`ctrl-c`. This is
  the literal truth of "global" the name already claimed.
- `HelpGroup::EntryOnly` (new): bindings that only do something on
  `Screen::Entry` — `d`, `r`, `o`, `s`, `ctrl-o`, `ctrl-i`. Applicable
  only when `!on_source_screen` (mirroring `HelpGroup::TreeFocus`/
  `HelpGroup::RightFocus`/`HelpGroup::Review`'s existing
  `is_group_applicable` shape, which already gates each on screen/focus
  reachability rather than always showing every group).

**9. `/`, `n`, `N` are added to `HelpGroup::SourceView`** (already
Source-screen-scoped, already filtered by `is_group_applicable`), rather
than a new group — they belong with the other Source-screen-only motion
bindings that group already lists (`j`/`k`, `Ctrl-d`/`Ctrl-u`, `gg`/`G`,
`Esc`/`q`).

Net effect: `Screen::Source`'s help view now shows `SourceView` (with
the three new search bindings) and `Global` (the narrowed, now-actually-
global set) — no more `EntryOnly` bindings advertised as pressable on a
screen where they are silently swallowed.

## Alternatives

- **Regex search.** Rejected for v1: adds a dependency and a failure
  mode (invalid pattern while composing) neither vim's own `/` nor this
  feature's actual driving need — "find this text" — requires. Could be
  added later as an opt-in prefix (e.g. a leading backslash) without
  breaking this ADR's literal-substring contract, if a concrete need
  surfaces.
- **Search across the Entry screen's Diff pane too, in this same ADR.**
  Rejected per Context above — the Diff pane's shaped/grouped content
  raises match-semantics questions (per-section vs. whole-file, and how
  a match interacts with ADR 0027's diff-focus auto-scroll) this ADR
  does not need to answer to ship Source-view search, and conflating the
  two would block the simpler, already-well-defined case on the harder
  one's design. Left as explicit future work.
- **A dedicated popup for the search buffer instead of the status
  line.** Rejected: a one-line buffer is exactly what the status line
  already exists to show (`App::status`'s transient-message precedent),
  and a popup would need to solve the same modal-priority-slot problem
  `ReviewMode::Compose` already solves — reusing that shape (decision 2)
  is strictly less new surface than a second popup type.
- **A new top-level `SearchState` enum embedded directly on `App` as
  several fields (query, mode flag, matches, cursor) rather than one
  struct.** Rejected: `ReviewState` already establishes the "one
  field on `App`, one owning module with its own transitions" pattern
  for exactly this kind of self-contained per-feature state (ADR
  0048's Module boundary decision) — `SearchState` follows that
  precedent instead of scattering four loose fields across `App`
  the way state looked before ADR 0048.
- **Recompute matches inside the render path (`crate::ui::source_screen`)
  on every draw.** Rejected on the same grounds ADR 0020's diff-shaping
  decision and this crate's `terminal.draw`-runs-on-idle-poll-ticks
  invariant already establish (`crate::event_loop::run_app`'s own doc
  comment on `diff_highlights`): matches are computed once, when the
  query changes or a search confirms, cached on `SearchState`, and the
  render path only reads the cached positions.
- **Case-insensitive `n`/`N` aliasing (accept both cases for either
  binding).** Rejected per this project's "no arbitrary case-insensitive
  keybindings" convention — see decision 4's justification for why `N`
  survives that convention as a distinct operation rather than an alias.

## Consequences

- `App` gains one new field, `search: SearchState` (mirroring
  `review: ReviewState`), and the priority-check chain in
  `App::handle_key` gains one more early-return branch, ordered
  correctly relative to the existing review/help/jump-popup/update-prompt
  checks (composing-mode input must not leak into any of those, the same
  way `ReviewMode::Compose` already guards itself first).
- `InputKey` gains `SearchStart`, `SearchChar(char)`, `SearchBackspace`,
  `SearchConfirm`, `SearchCancel`, `SearchNext`, `SearchPrev` — modeled
  one-for-one on the `NoteCompose`/`ComposeChar`/`ComposeBackspace`/
  `PopupConfirm`/`PopupCancel` precedent, kept as distinct variants
  (rather than reusing `ComposeChar`/`ComposeBackspace`) so a reviewer
  cannot end up composing a search query into a review note or vice versa
  through a shared variant that both modes happen to interpret.
- `crate::input_translate::translate_key` gains a `Screen::Source` +
  search-composing early return, structurally parallel to the existing
  `ReviewMode::Compose` early return at the top of the function.
- `source_screen.rs` gains the match-highlighting tint constants and a
  small amount of layering logic in `unchanged_line`; no new rendering
  mechanism.
- `help.rs`'s `HelpGroup` enum gains `EntryOnly`; `Global`'s binding list
  shrinks to the truly-global subset; `SourceView`'s binding list grows
  by three. `is_group_applicable`'s `Global => true` arm is joined by an
  `EntryOnly => !on_source_screen` arm.
- No backward-compatibility concern: the TUI has never shipped a release
  carrying these bindings, so `/`/`n`/`N` on the Source screen and the
  Global/EntryOnly split are both strict additions/corrections, not
  breaking changes to any real user.
- Future work (not this ADR): extending search to the Entry screen's
  Diff pane, and an opt-in regex mode.
