//! Raw unified-diff hunk view-model (TUI iteration 2, "diff" pane): given
//! the raw diff text `main.rs` already has in hand for every input mode
//! (stdin / `--base` / `--pr` — all three end up with a `String` of diff
//! text before `rinkaku_core::pipeline::analyze_diff` consumes it), slices
//! it into per-file hunks with styled lines, so a reviewer can see *what
//! changed* in the lines under a selected symbol/file, not just the
//! post-change signature.
//!
//! [`rinkaku_core::diff::parse_unified_diff`] already parses a unified diff
//! but only keeps the *line ranges* that changed (`ChangedFile`), not the
//! hunk text itself — that's all `rinkaku-core`'s extraction pipeline needs.
//! This module needs the actual `+`/`-`/context lines to render, which
//! `rinkaku-core`'s parser doesn't expose, so it re-walks the raw diff text
//! itself with a small, focused parser rather than reaching into
//! `rinkaku-core` for something it doesn't publish. This keeps `rinkaku-core`
//! untouched (CLAUDE.md: core logic changes are out of scope for this
//! feature) and keeps the parsing pure — a `&str` in, plain data out, no IO.

/// One line inside a hunk body, classified by its leading diff marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Added,
    Removed,
    Context,
}

/// One line of a hunk's body, with its marker stripped (`content` never
/// includes the leading `+`/`-`/` `) so a renderer can prepend its own
/// styled marker glyph rather than displaying git's raw prefix twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
}

/// One `@@ ... @@` hunk: its header text (shown dim, mirrors a diff tool's
/// own convention) and body lines, plus the new-side line range it covers —
/// used to test intersection against a symbol's [`rinkaku_core::diff::LineRange`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    /// 1-based inclusive new-side line range this hunk's `+`/context lines
    /// span. `None` when the hunk adds/removes zero new-side lines (a
    /// hunk that is pure deletion) — there is then no new-side range to
    /// intersect a symbol's line range against at all.
    pub new_range: Option<(usize, usize)>,
    pub lines: Vec<DiffLine>,
}

/// Every hunk touching one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHunks {
    /// New-side path (matches `rinkaku_core::render::FileReport::path`).
    pub path: String,
    pub hunks: Vec<Hunk>,
}

/// Parses raw unified diff text into per-file hunk blocks. Malformed input
/// (a hunk header this parser can't read) is skipped rather than erroring —
/// this view is a best-effort visual aid layered on top of the report the
/// core pipeline already successfully built from the same text, so a
/// parsing hiccup here should degrade to "no hunks shown for that file"
/// rather than taking down the whole TUI.
pub fn parse_diff_hunks(diff_text: &str) -> Vec<FileHunks> {
    let lines: Vec<&str> = diff_text.lines().collect();
    let mut files = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("diff --git ") {
            let (path, next) = extract_path(&lines, i);
            let (hunks, next) = parse_hunks(&lines, next);
            files.push(FileHunks { path, hunks });
            i = next;
        } else {
            i += 1;
        }
    }

    files
}

/// Extracts the new-side (`b/`) path from a `diff --git a/x b/y` header,
/// honoring a later `rename to`/`copy to` line the same way
/// `rinkaku_core::diff`'s own parser does, then returns the index of the
/// first hunk header (or the next file's header / end of input).
fn extract_path(lines: &[&str], start: usize) -> (String, usize) {
    let header = lines[start];
    let mut path = header
        .strip_prefix("diff --git ")
        .and_then(|rest| rest.find(" b/").map(|idx| rest[idx + 3..].to_string()))
        .unwrap_or_default();

    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("diff --git ") || line.starts_with("@@") {
            break;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            path = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("copy to ") {
            path = rest.to_string();
        }
        i += 1;
    }

    (path, i)
}

/// Parses every `@@ ... @@` hunk starting at `start`, stopping at the next
/// `diff --git` entry or end of input. Returns the parsed hunks and the
/// index following the last one consumed.
fn parse_hunks(lines: &[&str], start: usize) -> (Vec<Hunk>, usize) {
    let mut hunks = Vec::new();
    let mut i = start;

    while i < lines.len() && !lines[i].starts_with("diff --git ") {
        if lines[i].starts_with("@@") {
            let (hunk, next) = parse_one_hunk(lines, i);
            hunks.push(hunk);
            i = next;
        } else {
            i += 1;
        }
    }

    (hunks, i)
}

/// Parses one hunk starting at its `@@ -a,b +c,d @@` header line, returning
/// the hunk and the index following its body (the next `@@`, `diff --git`,
/// or end of input).
fn parse_one_hunk(lines: &[&str], start: usize) -> (Hunk, usize) {
    let header = lines[start];
    let new_start_count = parse_new_side_header(header);

    let mut body = Vec::new();
    let mut i = start + 1;
    while i < lines.len() && !lines[i].starts_with("@@") && !lines[i].starts_with("diff --git ") {
        let line = lines[i];
        if let Some(content) = line.strip_prefix('+') {
            body.push(DiffLine {
                kind: DiffLineKind::Added,
                content: content.to_string(),
            });
        } else if let Some(content) = line.strip_prefix('-') {
            body.push(DiffLine {
                kind: DiffLineKind::Removed,
                content: content.to_string(),
            });
        } else if line.starts_with('\\') {
            // "\ No newline at end of file" — not a content line, skipped
            // the same way rinkaku-core's own hunk parser skips it.
        } else {
            body.push(DiffLine {
                kind: DiffLineKind::Context,
                content: line.strip_prefix(' ').unwrap_or(line).to_string(),
            });
        }
        i += 1;
    }

    // The header's declared new-side count is a claim, not a fact —
    // `rinkaku_core::diff::parse_hunk` walks the body and errors out
    // (`HunkBodyMismatch`) when it doesn't match, but this module's parser
    // degrades instead of erroring (module doc comment), so an inflated
    // header count must not silently propagate into `new_range`: it would
    // let `hunks_for_range` match a symbol whose lines the hunk body never
    // actually touched. `Added` and `Context` lines are exactly the ones
    // that occupy a new-side line number; `Removed` lines don't.
    let actual_new_line_count = body
        .iter()
        .filter(|line| line.kind != DiffLineKind::Removed)
        .count();
    let new_range = new_start_count.and_then(|(start, declared_count)| {
        let count = declared_count.min(actual_new_line_count);
        if count == 0 {
            None
        } else {
            Some((start, start + count - 1))
        }
    });

    (
        Hunk {
            header: header.to_string(),
            new_range,
            lines: body,
        },
        i,
    )
}

/// Parses a `@@ -a,b +c,d @@` header's new side only, returning
/// `(start, count)`, or `None` when the header doesn't match the expected
/// shape at all (defensive — this view degrades to "no range to intersect"
/// for a hunk it can't read rather than erroring the whole parse).
fn parse_new_side_header(header: &str) -> Option<(usize, usize)> {
    let body = header.strip_prefix("@@ ")?.split(" @@").next()?;
    let new_part = body.split(' ').nth(1)?;
    let new_part = new_part.strip_prefix('+')?;
    let (start_str, count_str) = match new_part.split_once(',') {
        Some((s, c)) => (s, c),
        None => (new_part, "1"),
    };
    let start: usize = start_str.parse().ok()?;
    let count: usize = count_str.parse().ok()?;
    Some((start, count))
}

/// Whether `hunk` intersects the 1-based inclusive new-side range
/// `[range_start, range_end]` — used to filter a file's hunks down to just
/// the ones touching a selected symbol's line range.
pub fn hunk_intersects(hunk: &Hunk, range_start: usize, range_end: usize) -> bool {
    match hunk.new_range {
        Some((hunk_start, hunk_end)) => hunk_start <= range_end && range_start <= hunk_end,
        None => false,
    }
}

/// Every hunk in `file_hunks` intersecting `[range_start, range_end]`
/// (1-based inclusive, matching [`rinkaku_core::diff::LineRange`]'s own
/// convention) — the symbol-row view: "just the hunks touching this
/// symbol's lines", per the feature's own requirement that a symbol
/// selection show only intersecting hunks rather than the whole file's
/// diff.
pub fn hunks_for_range(file_hunks: &FileHunks, range_start: usize, range_end: usize) -> Vec<&Hunk> {
    file_hunks
        .hunks
        .iter()
        .filter(|hunk| hunk_intersects(hunk, range_start, range_end))
        .collect()
}

/// Finds the [`FileHunks`] for `path`, or `None` when the diff has no
/// entry for it (e.g. the file wasn't part of the diff at all, or the path
/// slipped through some other mismatch between `report` and `diff_text` —
/// defensive, since both are supposed to come from the same input).
pub fn file_hunks<'a>(files: &'a [FileHunks], path: &str) -> Option<&'a FileHunks> {
    files.iter().find(|f| f.path == path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_empty_vec_when_diff_text_is_empty() {
        let actual = parse_diff_hunks("");

        assert_eq!(Vec::<FileHunks>::new(), actual);
    }

    #[test]
    fn should_parse_one_file_with_one_hunk_and_mixed_line_kinds() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,3 @@
 fn a() {}
+fn b() {}
-fn old() {}
 fn c() {}
";
        let expected = vec![FileHunks {
            path: "src/lib.rs".to_string(),
            hunks: vec![Hunk {
                header: "@@ -1,3 +1,3 @@".to_string(),
                new_range: Some((1, 3)),
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: "fn a() {}".to_string(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        content: "fn b() {}".to_string(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Removed,
                        content: "fn old() {}".to_string(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: "fn c() {}".to_string(),
                    },
                ],
            }],
        }];
        let actual = parse_diff_hunks(diff);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_use_renamed_path_when_file_was_renamed_with_content_change() {
        let diff = "\
diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 90%
rename from src/old_name.rs
rename to src/new_name.rs
index e69de29..4b825dc 100644
--- a/src/old_name.rs
+++ b/src/new_name.rs
@@ -1,2 +1,3 @@
 fn a() {}
+fn b() {}
 fn c() {}
";
        let actual = parse_diff_hunks(diff);

        assert_eq!(1, actual.len());
        assert_eq!("src/new_name.rs", actual[0].path);
    }

    #[test]
    fn should_parse_multiple_hunks_in_one_file() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 fn a() {}
+fn b() {}
 fn c() {}
@@ -10,2 +11,3 @@
 fn x() {}
+fn y() {}
 fn z() {}
";
        let actual = parse_diff_hunks(diff);

        assert_eq!(1, actual.len());
        assert_eq!(2, actual[0].hunks.len());
        assert_eq!(Some((1, 3)), actual[0].hunks[0].new_range);
        assert_eq!(Some((11, 13)), actual[0].hunks[1].new_range);
    }

    #[test]
    fn should_parse_multiple_files_in_one_diff() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
index e69de29..4b825dc 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,1 +1,2 @@
 fn a() {}
+fn a2() {}
diff --git a/src/b.rs b/src/b.rs
index e69de29..4b825dc 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -1,1 +1,2 @@
 fn b() {}
+fn b2() {}
";
        let actual = parse_diff_hunks(diff);

        let paths: Vec<&str> = actual.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(vec!["src/a.rs", "src/b.rs"], paths);
    }

    #[test]
    fn should_ignore_no_newline_marker_line() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,2 @@
 fn a() {}
-fn b() {}
\\ No newline at end of file
+fn b2() {}
\\ No newline at end of file
";
        let actual = parse_diff_hunks(diff);

        assert_eq!(1, actual.len());
        let lines = &actual[0].hunks[0].lines;
        assert_eq!(3, lines.len());
        assert_eq!(DiffLineKind::Context, lines[0].kind);
        assert_eq!(DiffLineKind::Removed, lines[1].kind);
        assert_eq!(DiffLineKind::Added, lines[2].kind);
    }

    #[test]
    fn should_return_none_new_range_when_hunk_is_pure_deletion() {
        let diff = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
index 4b825dc..0000000
--- a/src/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn a() {}
-fn b() {}
";
        let actual = parse_diff_hunks(diff);

        assert_eq!(1, actual.len());
        assert_eq!(None, actual[0].hunks[0].new_range);
    }

    #[test]
    fn should_return_none_new_range_when_hunk_header_is_malformed() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ garbage @@
 fn a() {}
";
        let actual = parse_diff_hunks(diff);

        assert_eq!(1, actual.len());
        assert_eq!(None, actual[0].hunks[0].new_range);
    }

    // SHOULD-FIX: the header's declared new-side count is untrustworthy on
    // its own — this module's doc comment claims malformed input "degrades
    // to no hunks shown", but before this fix a header declaring more lines
    // than the body actually contains produced an inflated `new_range` that
    // could wrongly match unrelated symbols further down the file (see
    // `hunks_for_range`). The body's own actually-parsed new-side line count
    // must cap whatever the header claims.
    #[test]
    fn should_cap_new_range_when_hunk_body_is_shorter_than_declared_count() {
        let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,100 @@
 fn a() {}
+fn b() {}
";
        let actual = parse_diff_hunks(diff);

        // The header claims 100 new-side lines starting at 1, but the body
        // only actually contains 2 (one context, one added) — the range
        // must reflect the body's real extent, not the header's claim.
        assert_eq!(Some((1, 2)), actual[0].hunks[0].new_range);
    }

    #[test]
    fn should_report_hunk_intersects_when_ranges_overlap() {
        let hunk = Hunk {
            header: "@@ -1,3 +1,4 @@".to_string(),
            new_range: Some((5, 10)),
            lines: vec![],
        };

        assert_eq!(true, hunk_intersects(&hunk, 8, 20));
        assert_eq!(true, hunk_intersects(&hunk, 1, 5));
        assert_eq!(false, hunk_intersects(&hunk, 11, 20));
        assert_eq!(false, hunk_intersects(&hunk, 1, 4));
    }

    #[test]
    fn should_report_no_intersection_when_hunk_has_no_new_range() {
        let hunk = Hunk {
            header: "@@ -1,2 +0,0 @@".to_string(),
            new_range: None,
            lines: vec![],
        };

        assert_eq!(false, hunk_intersects(&hunk, 1, 100));
    }

    #[test]
    fn should_return_only_hunks_intersecting_the_given_range() {
        let file_hunks = FileHunks {
            path: "src/lib.rs".to_string(),
            hunks: vec![
                Hunk {
                    header: "@@ -1,1 +1,2 @@".to_string(),
                    new_range: Some((1, 2)),
                    lines: vec![],
                },
                Hunk {
                    header: "@@ -10,1 +11,2 @@".to_string(),
                    new_range: Some((11, 12)),
                    lines: vec![],
                },
            ],
        };

        let actual = hunks_for_range(&file_hunks, 11, 12);

        assert_eq!(1, actual.len());
        assert_eq!("@@ -10,1 +11,2 @@", actual[0].header);
    }

    #[test]
    fn should_find_file_hunks_by_path() {
        let files = vec![
            FileHunks {
                path: "a.rs".to_string(),
                hunks: vec![],
            },
            FileHunks {
                path: "b.rs".to_string(),
                hunks: vec![],
            },
        ];

        let actual = file_hunks(&files, "b.rs");

        assert_eq!(Some(&files[1]), actual);
    }

    #[test]
    fn should_return_none_when_path_not_found() {
        let files: Vec<FileHunks> = vec![];

        let actual = file_hunks(&files, "missing.rs");

        assert_eq!(None, actual);
    }
}
