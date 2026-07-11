//! Unified diff parsing.
//!
//! Parses unified diff text (as produced by `git diff` / `gh pr diff`) into
//! a structured list of changed files and the new-side line ranges that
//! changed. This is the input boundary for the tree-sitter extraction
//! pipeline: it tells that stage *which lines* changed, not *what* changed
//! syntactically.
//!
//! Implemented by hand rather than pulling in a `unidiff`-style crate: the
//! only thing this module needs is new-side line range computation from
//! hunk headers, which is a small, self-contained piece of parsing that
//! doesn't justify an extra dependency.

use thiserror::Error;

/// A contiguous, inclusive range of 1-based line numbers on the new side of
/// a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

/// The kind of change applied to a file in the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// A single file entry parsed out of a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// New-side path (the `b/` side), or the only path for deletions.
    pub path: String,
    /// Old-side path, present only when the file was renamed.
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    /// New-side line ranges containing added lines. Empty for deletions
    /// and for renames with no content change (pure rename, 100% similar).
    pub changed_ranges: Vec<LineRange>,
    /// True when git reported this as a binary file patch; `changed_ranges`
    /// is always empty in that case since there is no line-level diff.
    pub is_binary: bool,
}

/// Errors that can occur while parsing a unified diff.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("malformed hunk header: {0}")]
    MalformedHunkHeader(String),
    /// A hunk's body ended (because the next `diff --git` entry started, or
    /// input ran out) before the new-side line count declared in its
    /// `@@ ... @@` header was satisfied.
    #[error("hunk body does not match declared line count: {0}")]
    HunkBodyMismatch(String),
}

/// Parses unified diff text into a list of [`ChangedFile`] entries.
pub fn parse_unified_diff(input: &str) -> Result<Vec<ChangedFile>, ParseError> {
    let lines: Vec<&str> = input.lines().collect();
    let mut files = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("diff --git ") {
            let (file, next) = parse_file_entry(&lines, i)?;
            files.push(file);
            i = next;
        } else {
            i += 1;
        }
    }

    Ok(files)
}

/// Parses one `diff --git ...` entry (header + optional hunks) starting at
/// `start`. Returns the parsed [`ChangedFile`] and the index of the line
/// following this entry.
fn parse_file_entry(lines: &[&str], start: usize) -> Result<(ChangedFile, usize), ParseError> {
    let mut path = extract_git_header_paths(lines[start]).1;
    let mut old_path = None;
    let mut kind = ChangeKind::Modified;
    let mut is_binary = false;
    let mut i = start + 1;

    // Header lines (before the `--- ` / `+++ ` / `Binary files` marker)
    // carry rename/mode metadata that a `+++`/`---` pair alone can't
    // express (e.g. similarity index, explicit rename source/target).
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("diff --git ") {
            break;
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            old_path = Some(rest.to_string());
            kind = ChangeKind::Renamed;
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            path = rest.to_string();
            kind = ChangeKind::Renamed;
        } else if line.starts_with("new file mode") {
            kind = ChangeKind::Added;
        } else if line.starts_with("deleted file mode") {
            kind = ChangeKind::Deleted;
        } else if line.starts_with("Binary files ") {
            is_binary = true;
            i += 1;
            break;
        } else if line.starts_with("--- ") || line.starts_with("+++ ") {
            // The path pair is authoritative once present; skip to hunks.
        } else if line.starts_with("@@ ") {
            break;
        }
        i += 1;
    }

    let mut changed_ranges = Vec::new();
    while i < lines.len() && lines[i].starts_with("@@ ") {
        let (hunk_ranges, next) = parse_hunk(lines, i)?;
        changed_ranges.extend(hunk_ranges);
        i = next;
    }
    let changed_ranges = merge_adjacent_ranges(changed_ranges);

    Ok((
        ChangedFile {
            path,
            old_path,
            kind,
            changed_ranges,
            is_binary,
        },
        i,
    ))
}

/// Extracts the `a/...` and `b/...` paths from a `diff --git a/x b/y`
/// header line. Falls back to an empty string on unexpected input; the
/// authoritative path comes from `+++`/`rename to` lines parsed afterward.
fn extract_git_header_paths(line: &str) -> (String, String) {
    let rest = line.trim_start_matches("diff --git ");
    if let Some(b_idx) = rest.find(" b/") {
        let a = rest[..b_idx].trim_start_matches("a/").to_string();
        let b = rest[b_idx + 3..].to_string();
        (a, b)
    } else {
        (String::new(), String::new())
    }
}

/// Parses one `@@ -a,b +c,d @@` hunk starting at `start`, returning the
/// added-line ranges it contains and the index of the line following the
/// hunk body.
fn parse_hunk(lines: &[&str], start: usize) -> Result<(Vec<LineRange>, usize), ParseError> {
    let header = lines[start];
    let (mut new_line, new_count) = parse_hunk_header(header)?;
    let hunk_end_new_line = new_line
        .checked_add(new_count)
        .ok_or_else(|| ParseError::MalformedHunkHeader(header.to_string()))?;

    let mut ranges = Vec::new();
    let mut run_start: Option<usize> = None;
    let mut i = start + 1;

    while i < lines.len() && new_line < hunk_end_new_line {
        let line = lines[i];
        if line.starts_with("diff --git ") {
            // The hunk body ended (next file's header started) before the
            // declared new-side line count was reached. Treating this line
            // as context would silently swallow the next file's entry.
            return Err(ParseError::HunkBodyMismatch(header.to_string()));
        }
        if line.starts_with('+') {
            if run_start.is_none() {
                run_start = Some(new_line);
            }
            new_line += 1;
        } else if line.starts_with('\\') {
            // "\ No newline at end of file" - not a content line.
        } else {
            if let Some(s) = run_start.take() {
                ranges.push(LineRange {
                    start: s,
                    end: new_line - 1,
                });
            }
            if line.starts_with('-') {
                // Removed line: doesn't advance the new-side counter.
            } else {
                new_line += 1;
            }
        }
        i += 1;
    }
    if new_line < hunk_end_new_line {
        // Input ran out before the declared new-side line count was
        // reached.
        return Err(ParseError::HunkBodyMismatch(header.to_string()));
    }
    if let Some(s) = run_start.take() {
        ranges.push(LineRange {
            start: s,
            end: new_line - 1,
        });
    }

    Ok((ranges, i))
}

/// Parses a `@@ -a,b +c,d @@` header, returning the new-side start line and
/// line count. The old-side range is not needed by this module.
fn parse_hunk_header(header: &str) -> Result<(usize, usize), ParseError> {
    let malformed = || ParseError::MalformedHunkHeader(header.to_string());

    let body = header
        .strip_prefix("@@ ")
        .ok_or_else(malformed)?
        .split(" @@")
        .next()
        .ok_or_else(malformed)?;

    let mut parts = body.split(' ');
    let old = parts.next().ok_or_else(malformed)?;
    let new = parts.next().ok_or_else(malformed)?;
    let old = old.strip_prefix('-').ok_or_else(malformed)?;
    let new = new.strip_prefix('+').ok_or_else(malformed)?;

    // Old-side numbers are validated even though this module only needs
    // the new side: a header with a malformed old-side range is not a
    // well-formed hunk header and should be rejected rather than silently
    // accepted.
    parse_range(old).ok_or_else(malformed)?;
    let (start, count) = parse_range(new).ok_or_else(malformed)?;

    Ok((start, count))
}

/// Parses a `start[,count]` range as used on either side of a hunk header.
/// `count` defaults to `1` when omitted (git's shorthand for single-line
/// hunks).
fn parse_range(range: &str) -> Option<(usize, usize)> {
    let (start_str, count_str) = match range.split_once(',') {
        Some((s, c)) => (s, c),
        None => (range, "1"),
    };
    let start: usize = start_str.parse().ok()?;
    let count: usize = count_str.parse().ok()?;
    Some((start, count))
}

/// Merges ranges that are directly adjacent (`end + 1 == next.start`) into
/// a single range. Ranges are already produced in ascending order by the
/// hunk walk, so a single linear pass suffices.
fn merge_adjacent_ranges(ranges: Vec<LineRange>) -> Vec<LineRange> {
    let mut merged: Vec<LineRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if last.end + 1 == range.start => {
                last.end = range.end;
            }
            _ => merged.push(range),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_return_empty_vec_when_input_is_empty("")]
    fn parse_unified_diff_empty_cases(#[case] input: &str) {
        let expected: Vec<ChangedFile> = Vec::new();
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_single_added_range_when_modifying_one_hunk() {
        let input = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,5 @@
 fn a() {}
+fn b() {}
+fn c() {}
 fn d() {}
 fn e() {}
";
        let expected = vec![ChangedFile {
            path: "src/lib.rs".to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            changed_ranges: vec![LineRange { start: 2, end: 3 }],
            is_binary: false,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_merge_adjacent_added_lines_across_multiple_hunks() {
        let input = "\
diff --git a/src/lib.rs b/src/lib.rs
index e69de29..4b825dc 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 fn a() {}
+fn b() {}
 fn c() {}
 fn d() {}
@@ -10,2 +11,4 @@
 fn x() {}
+fn y() {}
+fn z() {}
 fn w() {}
";
        let expected = vec![ChangedFile {
            path: "src/lib.rs".to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            changed_ranges: vec![
                LineRange { start: 2, end: 2 },
                LineRange { start: 12, end: 13 },
            ],
            is_binary: false,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_mark_kind_added_when_file_is_new() {
        let input = "\
diff --git a/src/new.rs b/src/new.rs
new file mode 100644
index 0000000..4b825dc
--- /dev/null
+++ b/src/new.rs
@@ -0,0 +1,2 @@
+fn a() {}
+fn b() {}
";
        let expected = vec![ChangedFile {
            path: "src/new.rs".to_string(),
            old_path: None,
            kind: ChangeKind::Added,
            changed_ranges: vec![LineRange { start: 1, end: 2 }],
            is_binary: false,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_mark_kind_deleted_with_no_changed_ranges_when_file_is_removed() {
        let input = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
index 4b825dc..0000000
--- a/src/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn a() {}
-fn b() {}
";
        let expected = vec![ChangedFile {
            path: "src/old.rs".to_string(),
            old_path: None,
            kind: ChangeKind::Deleted,
            changed_ranges: vec![],
            is_binary: false,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_old_path_and_changed_ranges_when_file_is_renamed_with_content_change() {
        let input = "\
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
        let expected = vec![ChangedFile {
            path: "src/new_name.rs".to_string(),
            old_path: Some("src/old_name.rs".to_string()),
            kind: ChangeKind::Renamed,
            changed_ranges: vec![LineRange { start: 2, end: 2 }],
            is_binary: false,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_changed_ranges_when_rename_is_pure_with_no_hunks() {
        let input = "\
diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 100%
rename from src/old_name.rs
rename to src/new_name.rs
";
        let expected = vec![ChangedFile {
            path: "src/new_name.rs".to_string(),
            old_path: Some("src/old_name.rs".to_string()),
            kind: ChangeKind::Renamed,
            changed_ranges: vec![],
            is_binary: false,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_flag_is_binary_and_have_no_changed_ranges_when_file_is_binary() {
        let input = "\
diff --git a/assets/logo.png b/assets/logo.png
index e69de29..4b825dc 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
";
        let expected = vec![ChangedFile {
            path: "assets/logo.png".to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            changed_ranges: vec![],
            is_binary: true,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_ignore_no_newline_marker_when_computing_changed_ranges() {
        let input = "\
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
        let expected = vec![ChangedFile {
            path: "src/lib.rs".to_string(),
            old_path: None,
            kind: ChangeKind::Modified,
            changed_ranges: vec![LineRange { start: 2, end: 2 }],
            is_binary: false,
        }];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_parse_multiple_files_in_one_diff() {
        let input = "\
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
        let expected = vec![
            ChangedFile {
                path: "src/a.rs".to_string(),
                old_path: None,
                kind: ChangeKind::Modified,
                changed_ranges: vec![LineRange { start: 2, end: 2 }],
                is_binary: false,
            },
            ChangedFile {
                path: "src/b.rs".to_string(),
                old_path: None,
                kind: ChangeKind::Modified,
                changed_ranges: vec![LineRange { start: 2, end: 2 }],
                is_binary: false,
            },
        ];
        let actual = parse_unified_diff(input).expect("parse should succeed");
        assert_eq!(expected, actual);
    }

    // NOTE: partial assert here (`matches!` instead of a fully qualified
    // `assert_eq!`). `MalformedHunkHeader`'s payload is the raw offending
    // header text, so pinning its exact string would make these cases
    // brittle to unrelated wording tweaks; what matters for behavior is
    // that these inputs are rejected as malformed, not the exact message.
    #[rstest]
    #[case::should_error_when_hunk_header_is_missing_new_side_marker(
        "diff --git a/src/lib.rs b/src/lib.rs\nindex e69de29..4b825dc 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 1,4 @@\n fn a() {}\n"
    )]
    #[case::should_error_when_hunk_header_has_non_numeric_line_number(
        "diff --git a/src/lib.rs b/src/lib.rs\nindex e69de29..4b825dc 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -a,3 +1,4 @@\n fn a() {}\n"
    )]
    fn parse_unified_diff_malformed_hunk_header(#[case] input: &str) {
        let actual = parse_unified_diff(input);
        assert!(matches!(actual, Err(ParseError::MalformedHunkHeader(_))));
    }

    // Regression test for a bug where a hunk header's declared new_count was
    // larger than the actual hunk body, causing the parser to walk past the
    // end of the hunk and swallow the next file's `diff --git` line as a
    // context line. This made the second file disappear from the result
    // entirely instead of surfacing a parse error.
    #[test]
    fn should_return_err_when_hunk_body_is_shorter_than_declared_new_count() {
        let input = "\
diff --git a/src/a.rs b/src/a.rs
index e69de29..4b825dc 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,100 @@
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
        let actual = parse_unified_diff(input);
        assert!(matches!(actual, Err(ParseError::HunkBodyMismatch(_))));
    }

    // Regression test for a panic (debug build) / silent wraparound (release
    // build) when new_line + new_count overflows usize. A malformed but
    // syntactically valid-looking header must be rejected, not crash.
    #[test]
    fn should_return_err_when_hunk_header_line_numbers_overflow() {
        let input = "\
diff --git a/src/a.rs b/src/a.rs
index e69de29..4b825dc 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +18446744073709551615,18446744073709551615 @@
 fn a() {}
";
        let actual = parse_unified_diff(input);
        assert!(matches!(actual, Err(ParseError::MalformedHunkHeader(_))));
    }
}
