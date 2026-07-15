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
  your repository layout. Sibling directories order topologically over
  the change graph: entry-point directories (nothing else depends on
  them) sort first, heavily-depended-upon foundations sort last. Files
  within a directory are alphabetical; symbols keep source order. When
  a diff has no cross-directory references, ranking has nothing to
  work with and the order silently falls back to alphabetical — don't
  over-read ordering in that case. The underlying dependency edges come
  from syntactic tree-sitter resolution, not a type checker, so a
  reference can occasionally be missed.

  Rows carry badges: `chg:N` changed symbols, `api:N` contract changes
  (added / removed / signature-changed), `fan-in:N` other *production*
  symbols referencing this one — tests exercising it don't count, so
  the number reflects blast radius among changed code, not test
  coverage. `chg:`/`fan-in:` numbers are cyan (informational);
  `api:` is yellow, the same warning color as the file-size `warn:`
  badge, flagging it as the one badge worth a second look. A row whose
  badges show both a contract change and a high-fan-in symbol in its
  subtree gets a leading `!` (red, bold) — the combination that makes a
  change both hard to miss and wide-reaching; a signature-changed
  symbol with high fan-in of its own gets the same marker. Directories
  in a dependency cycle are marked `(cycle)`.

  Symbol rows show a kind abbreviation (`fn`, `struct`, ...) and a
  classification marker: `+` added, `~` signature-changed, `x` removed,
  or blank for body-only/unclassified — a blank-marker symbol's name
  also dims to DarkGray, since its signature didn't change and it
  carries less review weight. Files rinkaku couldn't analyze appear
  dimmed as `(skipped: <reason>)`; whole-test files as
  `[test] (N symbols)`. A *mixed* file's test symbols (real code
  alongside `#[cfg(test)]`-style tests in the same file) fold into a
  trailing `N tests` row (`1 test` singular), collapsed by default —
  expand it with `space`/`enter` to see them individually, dimmed and
  without any per-symbol badge, since group membership already says
  "this is test code".
- **Diff pane (right, default)** — the raw unified-diff hunks touching
  the selected row, syntax-highlighted for the four built-in languages
  ([ADR 0018](adr/0018-syntax-highlight-diff-pane-via-tree-sitter.md)).
  Opens in a split (side-by-side old/new) view by default — useful for
  edits where seeing the old and new lines aligned next to each other is
  easier to scan than interleaved `-`/`+` lines. Press `v`/`V` to switch
  to unified rendering for the same hunks. Split view needs a reasonably
  wide pane; on a narrow terminal it falls back to unified regardless of
  the toggle, and the pane header notes why.
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
| `v` / `V` | Toggle the Diff pane between unified and split (side-by-side) rendering |
| `s` / `S` | Open the source view for the symbol under the cursor |
| `gd` / `gr` | Jump to a callee / caller ([ADR 0022](adr/0022-jump-navigation-and-jumplist.md)) |
| `ctrl-o` / `ctrl-i` | Jump back / forward through the jumplist |
| `n` / `N` | Compose a review note / open the notes list ([ADR 0048](adr/0048-tui-review-actions.md)) |
| `w` / `W` | Open the current PR's page in a web browser, `--pr` mode only ([ADR 0050](adr/0050-tui-open-pr-in-browser.md)) |
| `U` | Prompt to self-update; opens automatically once a newer release is found, or reopens it after a dismissal ([ADR 0054](adr/0054-tui-update-available-prompt.md), [ADR 0056](adr/0056-tui-update-prompt-auto-open-and-freeze-fix.md)) |
| `?` | Toggle the help overlay |
| `q` / `ctrl-c` | Quit (from the entry view); `esc`/`q` also returns from the source view |

Glyphs are plain ASCII: `+` added, `~` signature-changed, `x` removed,
blank for body-only, `!` for the risk marker, `v`/`>` for expand state.

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
line range. In `--base` and stdin mode, this reads the working tree
(not the historical commit compared against), so the highlighted
range can drift if you edit the file after opening the TUI. In `--pr`
mode, this reads the PR's head snapshot instead (`git show <head
SHA>:<path>`), so it stays consistent with the diff regardless of
what's checked out locally. `esc`/`q` returns.

When the open file has diff hunks, they're overlaid directly onto the
full file: added lines get a green background and a `+` gutter,
removed lines appear as extra red-background rows with a `-` gutter
inserted at the position they used to occupy — the diff pane's
signal, but in the context of the whole file rather than clipped to
the changed lines alone. The overlay is always on; there's no key to
toggle it off. If the file's content doesn't match the diff (in
`--base`/stdin mode, edited since the diff was produced or diffed
against a different revision), the pane falls back to its plain,
unhighlighted rendering and says so in its title.

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
