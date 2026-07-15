# 0058. Rebind review notes from n/N to a/A, and rename "note" to "annotation"

- Status: Accepted
- Date: 2026-07-16

## Context

ADR 0048 bound the review-note feature to `n` (compose) / `N` (list) on
the Entry screen. ADR 0057 later added vim-like search to the Source
screen, also on `n`/`N` (next/previous match) — kept collision-free only
because ADR 0057 scoped the search bindings to `on_source_screen` and the
review-note bindings stay implicit "whatever screen is not Source"
(`Screen::Entry`). ADR 0057's own Alternatives section already flags
extending search to the Entry screen's Diff pane as future work; the day
that work starts, `n`/`N` cannot mean both "next search match" and
"compose/list review notes" on the same screen.

Rather than let that future work either re-litigate the review-note
binding under time pressure or invent a second pair of next/prev keys for
Entry-screen search, this ADR frees `n`/`N` now, while the rebind is a
mechanical change with a single caller (`crate::input_translate`) and no
in-flight branch depends on the old keys.

## Decision

**1. Review notes move from `n`/`N` to `a`/`A`** (`a` = annotate/compose,
`A` = list, mirroring the existing "uppercase is a distinct operation
from its lowercase counterpart, not a case-insensitive alias" convention
`n`/`N` and `gg`/`G` already use). `c` and `m` were both considered and
rejected: `c` is already `CollapseAll` on Tree focus, and `m` collides
with vim's own buffer-mark idiom, which would be a false affordance for
a project that otherwise borrows vim conventions on purpose. `a`/`A` is
free on every screen today.

**2. `n`/`N` are freed entirely by this ADR**, not reassigned — the
Entry screen simply has no `n`/`N` binding after this change. Reserving
them for a future Entry-screen search's next/prev match, mirroring the
Source screen's own `n`/`N` exactly, is the intended follow-up (ADR
0057's Alternatives), but implementing that search is out of scope here.

**3. The feature's name changes from "note" to "annotation"** throughout
user-visible text (help overlay, status line, overlay titles, both
locale files) and Rust identifiers (`InputKey::NoteCompose` →
`AnnotationCompose`, `NotesList` → `AnnotationsList`, `NoteDelete` →
`AnnotationDelete`; `review::Note`/`NoteLocation` →
`Annotation`/`AnnotationLocation`; `note_markers` module → shared
`Annotation` vocabulary throughout). `ReviewState`/`ReviewMode`/the
`review` module's own name are unchanged — "review" is the feature area
(ADR 0048's title), not the note vocabulary this ADR retires. The rename
is purely lexical (no behavior change): "annotation" was chosen over
keeping "note" because the key mnemonic (`a` for annotate) reads more
naturally against "annotation" than "note", and because a future
Entry-screen search feature reusing "note" terminology loosely (e.g. "a
note about a match") would be an unrelated, confusing overload once `n`
means search again.

## Alternatives

- **Keep `n`/`N` for review notes and give Entry-screen search a
  different pair (e.g. `f`/`F`) when that work starts.** Rejected: it
  would break the Source screen's `n`/`N` = next/prev muscle memory the
  moment a reviewer moves between Entry and Source, the exact
  inconsistency ADR 0057 avoided by scoping to `on_source_screen` in the
  first place. Freeing `n`/`N` now keeps that idiom uniform across both
  screens once Entry search ships.
- **Keep the "note" name and only change the key.** Rejected: leaving
  `NoteCompose` bound to `a` reads as an arbitrary mnemonic gap to any
  future reader of the code, whereas `AnnotationCompose` bound to `a` is
  self-explanatory without cross-referencing this ADR.
- **`c`/`m` for the new keys.** Rejected per Decision 1 — both already
  carry conflicting meaning (Tree-focus collapse, vim mark idiom).

## Consequences

- Breaking change for any reviewer with `n`/`N` muscle memory from ADR
  0048: those keys are silent no-ops on the Entry screen after this
  change, replaced by `a`/`A`.
- Entry-screen search can adopt `n`/`N` later without renegotiating this
  ADR's key choice.
- `rinkaku/src/notes.rs` and `AnalysisProgress::note` (unrelated
  diagnostic-message plumbing predating ADR 0048) are untouched — this
  ADR's "note" → "annotation" rename is scoped to the TUI review feature
  only, not every use of the word "note" in the codebase.
