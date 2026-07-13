# The interactive TUI

Markdown/JSON stay optimized for LLMs and CI ([ADR 0015](adr/0015-tui-for-humans-markdown-for-machines.md));
for a human reviewing a change in a terminal, pass `--tui` instead of
`--format` to open an interactive terminal UI ([ADR 0016](adr/0016-tui-crate-and-stack.md)),
built with [ratatui](https://ratatui.rs). For the flags controlling what
goes into the underlying report (`--deps`, `--include-tests`,
`--include-generated`, `--entry`), see
[CLI usage and output format](cli-usage.md).

```sh
rinkaku --base main --tui
```

`--tui` takes the same input flow as every other mode (stdin / `--base` /
`--pr`) and only changes the output stage, so it conflicts with `--format`
rather than combining with it. Bare `rinkaku`, run on an interactive
terminal with no `--base`/`--pr`, opens the TUI automatically on a
whole-repo outline instead of a diff ([ADR 0017](adr/0017-whole-repo-outline-as-default-input-mode.md));
the diff pane (the default right-hand pane, [ADR 0020](adr/0020-tui-interaction-model-v2.md))
has nothing to show in that case and renders a placeholder — press `d` to
switch to the detail pane instead.

## Interaction model

The interface follows a **focus** model ([ADR 0020](adr/0020-tui-interaction-model-v2.md)),
similar to neovim's window/pane idioms: at any moment, either the tree
(left pane) or the right-hand pane has focus, and `j`/`k` act on whichever
one does. `enter` on a file/symbol row moves focus to the right pane;
`h`/`esc` moves it back to the tree. Press `?` any time for an in-app
overlay listing every key and a short glossary (order modes, pivot,
cycle).

## What it shows

- **Entry pane (left):** the directory tree of changed files, not the
  call-graph tree — nesting depth mirrors your repository's layout.
  Directories and files carry badges: `~N` changed symbols, `!N` contract
  changes (added/removed/signature-changed), `^N` fan-in (hotspot
  aggregate). A directory that participates in a dependency cycle is
  marked `(cycle)`. By default, sibling directories are ordered
  topologically — entry points (least depended-on) first, foundations
  (most depended-on) last — the same shape the "Change graph" root-finding
  uses in Markdown, condensed to the directory level; `o` toggles to plain
  alphabetical order. Symbol rows show a kind abbreviation (`fn`, `struct`,
  ...) and a classification marker: `+` added, `~` signature-changed, `x`
  removed (dimmed and crossed out). A file rinkaku could not extract
  symbols from (unsupported language, binary, deleted) still shows up as a
  dimmed row marked `(skipped: <reason>)`, same reasons as `--format
  json`'s `skipped[].reason` — the same as Markdown's own "Skipped files"
  list, `SkipReason::Generated` entries are omitted by default (ADR
  0010/0011). A file whose changed symbols were *all* test code (ADR 0009)
  shows up too, marked `[test] (N symbols)`, instead of the pre-existing
  gap where such a file had no row at all and was only summarized in
  Markdown's "Tests" section.
- **Diff pane (right, the default):** the raw unified-diff hunks touching
  the selected row — "what changed" is what a reviewer wants to see first.
  A symbol row clips to just its own line range; a file row groups hunks
  under per-symbol section headers (each symbol's own signature line),
  with hunks matching no symbol (e.g. import-only changes) collected under
  a trailing `(module level)` section. When a symbol's signature itself
  changed, a 2-line old/new header (`- <old>` / `+ <new>`) precedes its
  hunks. A directory row has no single diff to show, since it spans
  multiple files. Hunks in the four built-in languages are
  syntax-highlighted (keywords, strings, types, ...); added/removed lines
  keep their green/red diff signal as a background tint so it doesn't
  compete with token colors, and any other file falls back to plain
  green/red text. `d`/`D` toggles the right-hand pane to the detail view
  instead.
- **Detail pane (right):** what the cursor is on. A symbol row shows its
  classification, signature (an old/new diff when the contract changed),
  who depends on it ("used by"), and its callees. A file row shows every
  symbol changed in that file with its classification marker and fan-in —
  or, for a skipped file, why rinkaku didn't analyze it; or, for a
  whole-test file, the changed test-symbol count. A directory row shows
  its badge breakdown and top fan-in symbols, plus — when it participates
  in a dependency cycle — exactly which other directories it cycles with
  and the concrete symbol-to-symbol edges forming that cycle.
- **Pivot pane (right):** `p`/`P` toggles the right-hand pane to an
  entry-tree view rooted at the directory or file row under the cursor
  ([ADR 0019](adr/0019-entry-path-pivot-view.md)) — the interactive
  equivalent of `--entry <path>` (see
  [CLI usage and output format](cli-usage.md#--entry-path)). The tree
  follows the cursor: move to a different directory/file row while
  pivoted and it re-renders rooted at the new row's path; a symbol row
  shows a placeholder instead, since pivoting only makes sense on a
  directory/file scope. Nodes reached only by expanding a dependency edge
  outward past the pivoted path are dimmed so you can tell "the layer I
  pivoted on" from "what it reaches into". Press `p` again, or `d`, to
  leave pivot mode.
- **Scrolling the right-hand pane:** move focus to the right pane
  (`enter` on a file/symbol row) and use `j`/`k` to scroll the
  Detail/Diff/Pivot pane down/up by one line when its content is too long
  to fit — the pane's title grows a `(first-last/total)` suffix (e.g.
  `Detail (1-17/43)`) whenever there's more to see, so a long cycle-edge
  list or a large file's diff doesn't quietly get cut off. While viewing
  the Diff pane specifically, `]`/`[` jump straight to the next/previous
  hunk. The scroll position resets to the top whenever the underlying
  content could have changed: moving the cursor, toggling between the
  detail/diff/pivot views, or returning from the source view.
- **Source view:** `s` on a symbol row opens that file, scrolled to and
  highlighting the symbol's line range; `esc`/`q` returns to the entry
  view. Reads the working tree directly (not the historical commit a
  `--base`/`--pr` diff was computed against), so it always shows the
  file's current content — note that the highlighted line range itself is
  from analysis time, so it can drift (or, if the file has since shrunk,
  get clamped to the end of the file) if you edit the file after opening
  the TUI.

## Key bindings

Press `?` in the TUI for the always-up-to-date version of this table,
grouped by focus.

**Tree focus (default):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Move the cursor |
| `enter` | Expand/collapse a directory row, or open a file/symbol row (moves focus right) |
| `space` | Expand or collapse a directory/file row (never moves focus) |
| `e` / `E` | Expand every row |
| `c` / `C` | Collapse every row |

**Right focus (Detail/Diff/Pivot pane):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Scroll the pane by one line |
| `h` / `esc` | Return focus to the tree |
| `]` | Jump to the next hunk (Diff pane only) |
| `[` | Jump to the previous hunk (Diff pane only) |

**Global (any focus):**

| Key(s) | Action |
| --- | --- |
| `o` / `O` | Toggle topological / alphabetical ordering |
| `d` / `D` | Toggle the right-hand pane between diff and detail |
| `p` / `P` | Toggle the right-hand pane to the pivot tree rooted at the selected directory/file |
| `s` / `S` | Open the source view for the symbol under the cursor |
| `?` | Toggle the help overlay (keymap + glossary) |
| `esc` / `q` | Return to the entry view (from the source view) |
| `q` / `ctrl-c` | Quit (from the entry view) |

Glyphs are plain ASCII (`~`/`!`/`^`/`+`/`x`, `v`/`>` for expand state)
rather than Unicode/emoji, for compatibility with plainer terminal
configurations.
