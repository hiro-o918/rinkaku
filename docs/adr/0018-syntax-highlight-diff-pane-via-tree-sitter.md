# 0018. Syntax-highlight the TUI diff pane via tree-sitter-highlight

- Status: accepted
- Date: 2026-07-13

## Context

The diff pane (`d`, since PR #51) renders hunks as whole-line green/red/
plain text. Users read code in that pane, and uniform per-line coloring
throws away the structure every editor gives them; the request for
syntax highlighting came from dogfooding. rinkaku already parses every
supported language with tree-sitter (ADR 0002), and each grammar crate
in use ships a `HIGHLIGHTS_QUERY`, so a highlighting stack is nearly
free — the design questions are which stack, what unit to parse, how to
compose token colors with the diff's added/removed signal, and where
the code lives.

A hunk is a slice of a file, not a parseable file, and single lines
parse even worse. But each hunk cleanly reconstructs into two texts:
the new side (context + added lines) and the old side (context +
removed lines), each of which is contiguous source the parser can
handle with tree-sitter's usual error tolerance.

## Decision

Highlight the diff pane with the `tree-sitter-highlight` crate and the
grammar crates' bundled `HIGHLIGHTS_QUERY`, entirely inside
`rinkaku-tui` (presentation concern; ADR 0015/0016 split —
`rinkaku-core`'s `LanguageSupport` is untouched, and the TUI keeps its
own path→grammar/query table for the four built-in languages). Per
hunk, reconstruct the new-side and old-side texts, highlight each side
once, and map token styles back to the display lines: token colors set
the foreground; the added/removed signal moves to a 256-color
background tint (dark green/red) with the `+`/`-` marker keeping its
bold foreground color; hunk headers stay dim. Highlighting runs once
per run alongside the existing once-per-run hunk parse (never inside
the render loop), and any failure — unknown extension, query error —
falls back per file to the current plain green/red styling.

## Alternatives

- **syntect (Sublime grammars), as delta uses**: mature highlighting,
  but introduces a second, regex-based parsing stack alongside
  tree-sitter in a project whose identity is tree-sitter. Rejected.
- **Per-line parsing**: simplest mapping, but single lines misparse
  constantly (unclosed delimiters, missing context). Rejected for
  quality; side-reconstruction costs little more.
- **Extending core's `LanguageSupport` with a highlight query**: keeps
  one language registry, but grows a core port for a purely
  presentational need. Rejected; revisit if the built-in language
  count grows enough that the TUI-side table hurts.

## Consequences

- The diff pane reads like an editor; body-only changes become easier
  to scan, which is the pane's whole purpose.
- Two new TUI-only dependencies (`tree-sitter-highlight`, the grammar
  crates moving into `rinkaku-tui`'s dependency set as well) and a
  small duplicated language table (4 entries) to keep core pure.
- Background tints assume a 256-color terminal; on true 16-color
  terminals the tint degrades but the `+`/`-` markers and gutter keep
  the diff signal legible.
- Old-side reconstruction means removed lines get highlighted in
  old-code context — correct, at the cost of parsing each hunk twice.
  Hunks are small; the once-per-run budget absorbs it.
