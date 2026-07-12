//! Syntax highlighting for the diff pane (ADR 0018).
//!
//! Kept in `rinkaku-tui`, not `rinkaku-core`: this is a presentation
//! concern layered on top of the raw hunks `crate::diff_view` already
//! parses, not a change to the extraction pipeline. `rinkaku-core`'s
//! `LanguageSupport` port stays untouched (ADR 0018's rejected
//! "extend core" alternative) — this module keeps its own small
//! extension -> grammar/query table for the same four built-in languages,
//! mirroring `rinkaku_core::language`'s registry style without depending
//! on it.
//!
//! Pipeline per hunk (`highlight_hunk`):
//! 1. Reconstruct the hunk's new-side text (context + added lines) and
//!    old-side text (context + removed lines) — a hunk itself is a slice
//!    of a file and does not parse well; each side is contiguous source
//!    tree-sitter's usual error tolerance can handle.
//! 2. Highlight each side once with `tree-sitter-highlight`.
//! 3. Map the resulting token spans back onto each display line's
//!    original position (new-side lines get their new-side highlight,
//!    old-side lines get their old-side highlight).
//!
//! Any failure along the way — unrecognized extension, query/parse error —
//! falls back to `None` per line, which `crate::ui` renders with the
//! existing plain green/red styling rather than losing the diff entirely.

use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

use crate::diff_view::{DiffLine, DiffLineKind, Hunk};

/// The token palette this module recognizes, in the order passed to
/// `HighlightConfiguration::configure` — a capture's resolved `Highlight`
/// index is a position into this slice, so `crate::ui`'s palette-to-style
/// lookup and this list must stay in lockstep: `crate::ui` resolves a
/// [`TokenSpan`]'s index back to its name via this slice before matching
/// on the name, so reordering entries here silently re-colors tokens even
/// though nothing fails to compile. A deliberately small subset of the capture
/// names the four bundled grammars' `HIGHLIGHTS_QUERY` files actually
/// emit that a reviewer benefits from distinguishing, not every capture
/// name `tree_sitter_highlight::STANDARD_CAPTURE_NAMES` documents.
pub const PALETTE: &[&str] = &[
    "keyword", "string", "comment", "function", "type", "number", "constant", "property",
    "variable",
];

/// One highlighted token span within a line, in byte offsets against that
/// line's own text (not the reconstructed side text) — `content` in
/// [`DiffLine`] terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSpan {
    pub start: usize,
    pub end: usize,
    /// Index into [`PALETTE`] naming which token kind this span is.
    pub palette_index: usize,
}

/// Per-line highlight result: `Some(spans)` when highlighting succeeded
/// for the file's language (an empty `Vec` is a valid, meaningful result —
/// a blank line has no tokens), `None` when this line's file/hunk could
/// not be highlighted at all (unknown extension, parse/query failure) —
/// `crate::ui` reads `None` as "fall back to the plain diff style for this
/// line".
pub type LineHighlight = Option<Vec<TokenSpan>>;

/// Looks up the tree-sitter grammar and highlights query for `path`'s
/// extension, mirroring `rinkaku_core::language::language_for_path`'s
/// extension-dispatch style without depending on that crate's registry
/// (ADR 0018: kept as a separate, TUI-only table).
fn config_for_path(path: &str) -> Option<HighlightConfiguration> {
    let extension = path.rsplit('.').next()?;
    let (language, highlights_query, injection_query, locals_query) = match extension {
        "rs" => (
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        ),
        "go" => (
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "py" => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "ts" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "tsx" => (
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        _ => return None,
    };

    let mut config = HighlightConfiguration::new(
        language,
        extension,
        highlights_query,
        injection_query,
        locals_query,
    )
    .ok()?;
    config.configure(PALETTE);
    Some(config)
}

/// Reconstructs a hunk's new-side text: every `Context`/`Added` line's
/// content, joined with `\n` — the text tree-sitter parses to highlight
/// what the file looks like *after* the change. Returns the joined text
/// plus, for each `Added`/`Context` line's original index in `hunk.lines`,
/// the byte range of that line's content within the joined text (so
/// highlight spans can be mapped back to the right display line).
fn reconstruct_side(
    lines: &[DiffLine],
    keep: DiffLineKind,
) -> (String, Vec<(usize, usize, usize)>) {
    let mut text = String::new();
    // (original line index, start byte, end byte) per kept line.
    let mut offsets = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let included = match keep {
            DiffLineKind::Added => matches!(line.kind, DiffLineKind::Added | DiffLineKind::Context),
            DiffLineKind::Removed => {
                matches!(line.kind, DiffLineKind::Removed | DiffLineKind::Context)
            }
            DiffLineKind::Context => unreachable!("keep is always Added or Removed"),
        };
        if !included {
            continue;
        }

        let start = text.len();
        text.push_str(&line.content);
        let end = text.len();
        offsets.push((index, start, end));
        text.push('\n');
    }

    (text, offsets)
}

/// Highlights `text` with `config`, returning the resolved token spans in
/// byte-offset order, or `None` on any parse/highlight failure — the
/// caller's cue to fall back to the plain diff style for every line this
/// text covers.
fn highlight_text(config: &HighlightConfiguration, text: &str) -> Option<Vec<TokenSpan>> {
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, text.as_bytes(), None, |_| None)
        .ok()?;

    let mut spans = Vec::new();
    let mut active: Vec<Highlight> = Vec::new();
    for event in events {
        match event.ok()? {
            HighlightEvent::HighlightStart(highlight) => active.push(highlight),
            HighlightEvent::HighlightEnd => {
                active.pop();
            }
            HighlightEvent::Source { start, end } => {
                // The innermost (most specific) active capture wins when
                // queries nest — tree-sitter-highlight already resolves
                // overlapping captures via its own precedence rules before
                // emitting events, so the last-pushed (topmost) highlight
                // on the stack is the one to render.
                if let Some(highlight) = active.last() {
                    spans.push(TokenSpan {
                        start,
                        end,
                        palette_index: highlight.0,
                    });
                }
            }
        }
    }

    Some(spans)
}

/// Slices `spans` (byte offsets into the reconstructed side text) down to
/// the sub-range `[line_start, line_end)` belonging to one display line,
/// rebasing each span's offsets to be relative to that line's own text —
/// the coordinate space [`TokenSpan`] documents.
fn spans_for_line(spans: &[TokenSpan], line_start: usize, line_end: usize) -> Vec<TokenSpan> {
    spans
        .iter()
        .filter_map(|span| {
            let start = span.start.max(line_start);
            let end = span.end.min(line_end);
            if start >= end {
                return None;
            }
            Some(TokenSpan {
                start: start - line_start,
                end: end - line_start,
                palette_index: span.palette_index,
            })
        })
        .collect()
}

/// Highlights every line of `hunk`, keyed by `path`'s extension, returning
/// one [`LineHighlight`] per entry in `hunk.lines` (same length, same
/// order). Falls back to `vec![None; hunk.lines.len()]` when `path`'s
/// extension is unrecognized or either side fails to highlight — the
/// whole hunk degrades together rather than partially, since a
/// parse/query failure on one side says nothing useful about whether the
/// other would have looked visually consistent next to it.
pub fn highlight_hunk(path: &str, hunk: &Hunk) -> Vec<LineHighlight> {
    let fallback = || vec![None; hunk.lines.len()];

    let Some(config) = config_for_path(path) else {
        return fallback();
    };

    let (new_text, new_offsets) = reconstruct_side(&hunk.lines, DiffLineKind::Added);
    let (old_text, old_offsets) = reconstruct_side(&hunk.lines, DiffLineKind::Removed);

    let Some(new_spans) = highlight_text(&config, &new_text) else {
        return fallback();
    };
    let Some(old_spans) = highlight_text(&config, &old_text) else {
        return fallback();
    };

    let mut result: Vec<LineHighlight> = vec![None; hunk.lines.len()];
    for (index, start, end) in new_offsets {
        result[index] = Some(spans_for_line(&new_spans, start, end));
    }
    for (index, start, end) in old_offsets {
        result[index] = Some(spans_for_line(&old_spans, start, end));
    }

    result
}

/// One file's hunks, each already highlighted — `hunks[i]` corresponds
/// positionally to `crate::diff_view::FileHunks::hunks[i]` for the same
/// path (see [`highlight_diff_files`]'s doc comment on why this crate
/// keeps the association by position/pointer rather than growing
/// `crate::diff_view::Hunk` itself with a highlight field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedFile {
    pub path: String,
    pub hunks: Vec<Vec<LineHighlight>>,
}

/// Highlights every hunk of every file in `files` once. Called exactly
/// once per TUI session, immediately after `crate::diff_view::parse_diff_hunks`
/// (`crate::run_app`'s doc comment on why parsing itself is once-per-run,
/// not per-frame) — highlighting is strictly more expensive than parsing
/// (a full tree-sitter parse per hunk side, twice), so it must not run
/// inside the render loop either.
///
/// Kept as a separate, parallel `Vec<HighlightedFile>` rather than adding
/// a highlight field onto `crate::diff_view::Hunk` itself: `diff_view` is
/// a pure, dependency-free diff-text parser predating this feature, and
/// this module is the one new dependency on `tree-sitter-highlight` (ADR
/// 0018) — keeping them separate means a parse failure in this module can
/// never affect `diff_view`'s own parsing, and `diff_view`'s existing
/// tests stay untouched.
pub fn highlight_diff_files(files: &[crate::diff_view::FileHunks]) -> Vec<HighlightedFile> {
    files
        .iter()
        .map(|file| HighlightedFile {
            path: file.path.clone(),
            hunks: file
                .hunks
                .iter()
                .map(|hunk| highlight_hunk(&file.path, hunk))
                .collect(),
        })
        .collect()
}

/// Looks up the precomputed highlight for `hunk`, matching it against
/// `highlighted.hunks` by pointer identity against `file_hunks.hunks`
/// (rather than by value/`header` text, which is not guaranteed unique
/// within a file) — valid because every `&Hunk` `crate::ui` ever holds for
/// a given file (via `crate::diff_view::hunks_for_range` or a direct
/// `file.hunks.iter()`) is borrowed straight through from that same
/// `FileHunks`, never cloned, so its address always matches one entry of
/// `file_hunks.hunks`. Returns `None` when no match is found (defensive —
/// would only happen if a caller passed a `Hunk` from a different
/// `FileHunks` than the one `highlighted` was computed from) or when
/// `highlighted` itself is `None` (path not found in the precomputed
/// set), both of which fall back to the plain diff style exactly like an
/// unrecognized extension does.
pub fn lookup_hunk_highlight<'a>(
    highlighted: Option<&'a HighlightedFile>,
    file_hunks: &crate::diff_view::FileHunks,
    hunk: &Hunk,
) -> Option<&'a [LineHighlight]> {
    let highlighted = highlighted?;
    let index = file_hunks
        .hunks
        .iter()
        .position(|candidate| std::ptr::eq(candidate, hunk))?;
    highlighted.hunks.get(index).map(|lines| lines.as_slice())
}

/// Finds the [`HighlightedFile`] for `path` in the precomputed set, or
/// `None` when the diff has no entry for it — mirrors
/// `crate::diff_view::file_hunks`'s own lookup-by-path convention.
pub fn highlighted_file<'a>(
    files: &'a [HighlightedFile],
    path: &str,
) -> Option<&'a HighlightedFile> {
    files.iter().find(|f| f.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_view::DiffLineKind;
    use pretty_assertions::assert_eq;

    fn line(kind: DiffLineKind, content: &str) -> DiffLine {
        DiffLine {
            kind,
            content: content.to_string(),
        }
    }

    // --- reconstruct_side ---

    #[test]
    fn should_reconstruct_new_side_from_context_and_added_lines_only() {
        let lines = vec![
            line(DiffLineKind::Context, "fn a() {}"),
            line(DiffLineKind::Added, "fn b() {}"),
            line(DiffLineKind::Removed, "fn old() {}"),
            line(DiffLineKind::Context, "fn c() {}"),
        ];

        let (text, offsets) = reconstruct_side(&lines, DiffLineKind::Added);

        assert_eq!("fn a() {}\nfn b() {}\nfn c() {}\n", text);
        assert_eq!(vec![(0, 0, 9), (1, 10, 19), (3, 20, 29)], offsets);
    }

    #[test]
    fn should_reconstruct_old_side_from_context_and_removed_lines_only() {
        let lines = vec![
            line(DiffLineKind::Context, "fn a() {}"),
            line(DiffLineKind::Added, "fn b() {}"),
            line(DiffLineKind::Removed, "fn old() {}"),
            line(DiffLineKind::Context, "fn c() {}"),
        ];

        let (text, offsets) = reconstruct_side(&lines, DiffLineKind::Removed);

        assert_eq!("fn a() {}\nfn old() {}\nfn c() {}\n", text);
        assert_eq!(vec![(0, 0, 9), (2, 10, 21), (3, 22, 31)], offsets);
    }

    #[test]
    fn should_return_empty_text_when_no_lines_match_the_requested_side() {
        let lines = vec![line(DiffLineKind::Added, "fn b() {}")];

        let (text, offsets) = reconstruct_side(&lines, DiffLineKind::Removed);

        assert_eq!("", text);
        assert_eq!(Vec::<(usize, usize, usize)>::new(), offsets);
    }

    // --- spans_for_line ---

    #[test]
    fn should_rebase_and_clip_spans_to_the_requested_line_range() {
        let spans = vec![
            TokenSpan {
                start: 0,
                end: 2,
                palette_index: 0,
            },
            TokenSpan {
                start: 5,
                end: 9,
                palette_index: 1,
            },
            TokenSpan {
                start: 20,
                end: 25,
                palette_index: 2,
            },
        ];

        let actual = spans_for_line(&spans, 5, 10);

        assert_eq!(
            vec![TokenSpan {
                start: 0,
                end: 4,
                palette_index: 1,
            }],
            actual
        );
    }

    #[test]
    fn should_return_empty_when_no_span_overlaps_the_line_range() {
        let spans = vec![TokenSpan {
            start: 0,
            end: 2,
            palette_index: 0,
        }];

        let actual = spans_for_line(&spans, 5, 10);

        assert_eq!(Vec::<TokenSpan>::new(), actual);
    }

    // --- highlight_hunk: fallback ---

    #[test]
    fn should_fall_back_to_all_none_when_extension_is_unrecognized() {
        let hunk = Hunk {
            header: "@@ -1,1 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![line(DiffLineKind::Context, "some: yaml")],
        };

        let actual = highlight_hunk("config.yaml", &hunk);

        assert_eq!(vec![None], actual);
    }

    #[test]
    fn should_fall_back_to_all_none_when_path_has_no_extension() {
        let hunk = Hunk {
            header: "@@ -1,1 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![line(DiffLineKind::Context, "text")],
        };

        let actual = highlight_hunk("Makefile", &hunk);

        assert_eq!(vec![None], actual);
    }

    // --- highlight_hunk: real highlighting ---

    #[test]
    fn should_highlight_rust_keyword_and_string_tokens_in_an_added_line() {
        let hunk = Hunk {
            header: "@@ -0,0 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![line(DiffLineKind::Added, r#"fn foo() { let x = "s"; }"#)],
        };

        let actual = highlight_hunk("src/lib.rs", &hunk);

        assert_eq!(1, actual.len());
        let spans = actual[0]
            .clone()
            .expect("expected Some(spans) for a .rs file");

        // NOTE: partial assert — pinning every punctuation/variable span
        // tree-sitter-highlight's Rust query happens to emit would make
        // this test brittle to upstream query-file changes; instead this
        // asserts that the two tokens central to this feature's value
        // (`fn` as a keyword, `"s"` as a string) both resolved to their
        // expected palette entries at their expected byte offsets, which
        // is the guarantee `crate::ui` actually depends on.
        let keyword_index = PALETTE.iter().position(|p| *p == "keyword").unwrap();
        let string_index = PALETTE.iter().position(|p| *p == "string").unwrap();

        let text = r#"fn foo() { let x = "s"; }"#;
        let fn_start = text.find("fn").unwrap();
        let fn_span = spans
            .iter()
            .find(|s| s.start == fn_start)
            .expect("expected a span starting at 'fn'");
        assert_eq!(fn_start + 2, fn_span.end);
        assert_eq!(keyword_index, fn_span.palette_index);

        let string_start = text.find("\"s\"").unwrap();
        let string_span = spans
            .iter()
            .find(|s| s.start == string_start)
            .expect("expected a span starting at the string literal");
        assert_eq!(string_start + 3, string_span.end);
        assert_eq!(string_index, string_span.palette_index);
    }

    #[test]
    fn should_highlight_removed_line_in_old_side_context() {
        let hunk = Hunk {
            header: "@@ -1,1 +0,0 @@".to_string(),
            new_range: None,
            lines: vec![line(DiffLineKind::Removed, "fn foo() {}")],
        };

        let actual = highlight_hunk("src/lib.rs", &hunk);

        assert_eq!(1, actual.len());
        let spans = actual[0]
            .clone()
            .expect("expected Some(spans) for a .rs file");
        let keyword_index = PALETTE.iter().position(|p| *p == "keyword").unwrap();
        assert!(
            spans
                .iter()
                .any(|s| s.start == 0 && s.end == 2 && s.palette_index == keyword_index)
        );
    }

    #[test]
    fn should_highlight_go_keyword_tokens_in_an_added_line() {
        let hunk = Hunk {
            header: "@@ -0,0 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![line(DiffLineKind::Added, "func foo() {}")],
        };

        let actual = highlight_hunk("main.go", &hunk);

        let spans = actual[0]
            .clone()
            .expect("expected Some(spans) for a .go file");
        let keyword_index = PALETTE.iter().position(|p| *p == "keyword").unwrap();
        assert!(
            spans
                .iter()
                .any(|s| s.start == 0 && s.end == 4 && s.palette_index == keyword_index)
        );
    }

    #[test]
    fn should_highlight_python_keyword_tokens_in_an_added_line() {
        let hunk = Hunk {
            header: "@@ -0,0 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![line(DiffLineKind::Added, "def foo(): pass")],
        };

        let actual = highlight_hunk("main.py", &hunk);

        let spans = actual[0]
            .clone()
            .expect("expected Some(spans) for a .py file");
        let keyword_index = PALETTE.iter().position(|p| *p == "keyword").unwrap();
        assert!(
            spans
                .iter()
                .any(|s| s.start == 0 && s.end == 3 && s.palette_index == keyword_index)
        );
    }

    #[test]
    fn should_highlight_typescript_keyword_and_type_tokens_in_an_added_line() {
        // "function"/"const"/"if"/"return" are plain JavaScript keywords:
        // `tree-sitter-typescript`'s own `HIGHLIGHTS_QUERY` only adds the
        // TS-specific extras on top of a JS highlights query this grammar
        // crate does not bundle (`tree-sitter-javascript` is a separate,
        // undeclared dependency), so this crate's query captures
        // TS-specific keywords like `interface` but not `function`
        // (confirmed by inspecting `queries/highlights.scm` directly).
        // This test asserts on what the query actually recognizes rather
        // than a plain-JS keyword it would silently fail to highlight.
        let hunk = Hunk {
            header: "@@ -0,0 +1,1 @@".to_string(),
            new_range: Some((1, 1)),
            lines: vec![line(DiffLineKind::Added, "interface Foo { x: string; }")],
        };

        let actual = highlight_hunk("main.ts", &hunk);

        let spans = actual[0]
            .clone()
            .expect("expected Some(spans) for a .ts file");
        let keyword_index = PALETTE.iter().position(|p| *p == "keyword").unwrap();
        let type_index = PALETTE.iter().position(|p| *p == "type").unwrap();
        assert!(
            spans
                .iter()
                .any(|s| s.start == 0 && s.end == 9 && s.palette_index == keyword_index)
        );
        let string_type_start = "interface Foo { x: ".len();
        assert!(spans.iter().any(|s| s.start == string_type_start
            && s.end == string_type_start + 6
            && s.palette_index == type_index));
    }

    #[test]
    fn should_return_empty_spans_for_a_blank_context_line() {
        let hunk = Hunk {
            header: "@@ -1,2 +1,2 @@".to_string(),
            new_range: Some((1, 2)),
            lines: vec![
                line(DiffLineKind::Context, "fn a() {}"),
                line(DiffLineKind::Context, ""),
            ],
        };

        let actual = highlight_hunk("src/lib.rs", &hunk);

        assert_eq!(2, actual.len());
        assert_eq!(Some(Vec::new()), actual[1].clone());
    }

    // --- highlight_diff_files / lookup_hunk_highlight ---

    use crate::diff_view::FileHunks;

    #[test]
    fn should_highlight_every_hunk_of_every_file_when_computing_the_whole_diff() {
        let files = vec![
            FileHunks {
                path: "src/lib.rs".to_string(),
                hunks: vec![Hunk {
                    header: "@@ -0,0 +1,1 @@".to_string(),
                    new_range: Some((1, 1)),
                    lines: vec![line(DiffLineKind::Added, "fn a() {}")],
                }],
            },
            FileHunks {
                path: "config.yaml".to_string(),
                hunks: vec![Hunk {
                    header: "@@ -0,0 +1,1 @@".to_string(),
                    new_range: Some((1, 1)),
                    lines: vec![line(DiffLineKind::Added, "key: value")],
                }],
            },
        ];

        let actual = highlight_diff_files(&files);

        assert_eq!(2, actual.len());
        assert_eq!("src/lib.rs", actual[0].path);
        // .rs highlights successfully (Some), .yaml falls back (None) —
        // pins that per-file extension dispatch survives the whole-diff
        // batch path, not just the single-hunk `highlight_hunk` path.
        assert!(actual[0].hunks[0][0].is_some());
        assert_eq!(None, actual[1].hunks[0][0]);
    }

    #[test]
    fn should_look_up_highlight_by_hunk_pointer_identity() {
        let file_hunks = FileHunks {
            path: "src/lib.rs".to_string(),
            hunks: vec![
                Hunk {
                    header: "@@ -0,0 +1,1 @@".to_string(),
                    new_range: Some((1, 1)),
                    lines: vec![line(DiffLineKind::Added, "fn a() {}")],
                },
                Hunk {
                    header: "@@ -10,0 +11,1 @@".to_string(),
                    new_range: Some((11, 11)),
                    lines: vec![line(DiffLineKind::Added, "fn b() {}")],
                },
            ],
        };
        let highlighted = highlight_diff_files(std::slice::from_ref(&file_hunks));

        // Look up the *second* hunk specifically, to pin that the index
        // returned matches the hunk passed in, not just hunk 0.
        let target = &file_hunks.hunks[1];
        let actual = lookup_hunk_highlight(
            highlighted_file(&highlighted, "src/lib.rs"),
            &file_hunks,
            target,
        );

        assert_eq!(Some(highlighted[0].hunks[1].as_slice()), actual);
    }

    #[test]
    fn should_return_none_when_highlighted_file_is_absent() {
        let file_hunks = FileHunks {
            path: "src/lib.rs".to_string(),
            hunks: vec![Hunk {
                header: "@@ -0,0 +1,1 @@".to_string(),
                new_range: Some((1, 1)),
                lines: vec![line(DiffLineKind::Added, "fn a() {}")],
            }],
        };

        let actual = lookup_hunk_highlight(None, &file_hunks, &file_hunks.hunks[0]);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_find_highlighted_file_by_path() {
        let files = vec![HighlightedFile {
            path: "src/lib.rs".to_string(),
            hunks: vec![],
        }];

        let actual = highlighted_file(&files, "src/lib.rs");

        assert_eq!(Some(&files[0]), actual);
    }

    #[test]
    fn should_return_none_when_highlighted_file_path_not_found() {
        let files: Vec<HighlightedFile> = vec![];

        let actual = highlighted_file(&files, "missing.rs");

        assert_eq!(None, actual);
    }
}
