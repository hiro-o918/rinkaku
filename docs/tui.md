# Interactive TUI

The TUI is the intended path for humans reading a PR
([ADR 0015](adr/0015-tui-for-humans-markdown-for-machines.md)), built
with [ratatui](https://ratatui.rs)
([ADR 0016](adr/0016-tui-crate-and-stack.md)) on a neovim-style focus
model ([ADR 0020](adr/0020-tui-interaction-model-v2.md)).

On an interactive terminal, TUI is the **default output**: bare
`rinkaku` opens the TUI on a whole-repo outline
([ADR 0017](adr/0017-whole-repo-outline-as-default-input-mode.md)), and
so do `rinkaku --pr 123`, `rinkaku --base main`, etc. The `--tui` flag
only becomes load-bearing when the output would otherwise not be TUI
(stdout is not a TTY, e.g. a diff piped in). It conflicts with
`--format`.

## Layout

- **Tree pane (left)** — a directory tree of *changed files*, mirroring
  your repository layout. Rows carry badges: `~N` changed symbols, `!N`
  contract changes (added / removed / signature-changed), `^N` fan-in
  (hotspot aggregate); directories in a dependency cycle are marked
  `(cycle)`. Symbol rows show a kind abbreviation (`fn`, `struct`, ...)
  and a classification marker: `+` added, `~` signature-changed, `x`
  removed. Files rinkaku couldn't analyze appear dimmed as
  `(skipped: <reason>)`; whole-test files as `[test] (N symbols)`.
- **Diff pane (right, default)** — the raw unified-diff hunks touching
  the selected row, syntax-highlighted for the four built-in languages
  ([ADR 0018](adr/0018-syntax-highlight-diff-pane-via-tree-sitter.md)).
- **Detail pane (right)** — what the cursor is on: classification,
  signature (with an old/new diff on contract change), used-by, callees;
  or, for a directory, its badge breakdown and cycle members.
- **Blast radius pane (right)** — an entry-tree rooted at the cursor's
  directory/file: *"if this changes, what does it reach?"* The
  interactive equivalent of `--entry <path>`
  ([ADR 0019](adr/0019-entry-path-pivot-view.md), named per
  [ADR 0023](adr/0023-tui-blast-radius-naming.md)).

## Key bindings

Press `?` in the TUI for the always-up-to-date table.

**Tree focus (default):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Move the cursor |
| `enter` | Expand/collapse a directory, or open a file/symbol row (moves focus right) |
| `space` | Expand or collapse (never moves focus) |
| `e` / `E` | Expand every row |
| `c` / `C` | Collapse every row |

**Right focus (Detail / Diff / Blast-radius pane):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Scroll by one line |
| `ctrl-d` / `ctrl-u` | Half-page scroll |
| `gg` / `G` | Jump to top / bottom |
| `h` / `esc` | Return focus to the tree |
| `]` / `[` | Next / previous hunk (Diff pane only) |

**Global (any focus):**

| Key(s) | Action |
| --- | --- |
| `o` / `O` | Toggle topological / alphabetical ordering |
| `d` / `D` | Toggle right pane between diff and detail |
| `r` / `R` | Toggle right pane to blast radius |
| `s` / `S` | Open the source view for the symbol under the cursor |
| `gd` / `gr` | Jump to a callee / caller ([ADR 0022](adr/0022-jump-navigation-and-jumplist.md)) |
| `ctrl-o` / `ctrl-i` | Jump back / forward through the jumplist |
| `?` | Toggle the help overlay |
| `q` / `ctrl-c` | Quit (from the entry view); `esc`/`q` also returns from the source view |

Glyphs are plain ASCII (`~`/`!`/`^`/`+`/`x`, `v`/`>` for expand state).

## Jump navigation (`gd` / `gr`)

`gd` jumps toward a callee of the symbol under the cursor, `gr`
toward a caller — a two-key sequence, mirroring neovim's own idiom
([ADR 0022](adr/0022-jump-navigation-and-jumplist.md)). Zero candidates
shows a status-line note, one candidate jumps immediately, more than
one opens a popup (`j`/`k` to choose, `enter` to jump, `esc` to
cancel). Every jump is recorded in a jumplist; `ctrl-o` / `ctrl-i`
move back / forward, and a new jump from mid-history discards forward
entries — same behavior as neovim's jumplist.

## Source view

`s` on a symbol row opens the file scrolled to and highlighting its
line range. Reads the working tree (not the historical commit
`--base`/`--pr` compared against), so the highlighted range can drift
if you edit the file after opening the TUI. `esc`/`q` returns.

## Combining with `--entry`

`--entry <path>` combined with the TUI opens with the cursor already on
the tree row matching `path` and the Blast-radius pane already active
(the `R` binding) — the interactive session starts exactly where
`--entry` would have rooted the Markdown/JSON tree. If no row's path
matches exactly, the TUI opens normally with a status-line note.

## Piped stdin + TUI

Keyboard input is read from the controlling terminal independently of
stdin ([ADR 0016](adr/0016-tui-crate-and-stack.md)'s addendum), so a
piped diff and interactive navigation coexist:

```sh
gh pr diff 123 | rinkaku --tui
```

Here `--tui` is required because stdout may not be a TTY under the pipe
composition.
