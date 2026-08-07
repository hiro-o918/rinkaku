//! Changed-line counts for files with no changed symbol (ADR 0070).
//!
//! Pure over `(path, changed_line_count)` pairs — [`crate::pipeline`]
//! collects the pairs while it already holds each file's `changed_ranges`
//! in scope (a file only contributes a pair here when its `FileReport`
//! ends up with an empty `symbols` list; see `analyze_diff`'s doc
//! comment). This mirrors [`crate::file_size`]'s pair-in,
//! `Vec<Entry>`-out shape.

use serde::Serialize;

/// One file's total changed-line count, for a file whose diff produced no
/// changed symbol (ADR 0070) — reported on
/// [`crate::render::Report::non_symbol_changes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NonSymbolChange {
    pub path: String,
    pub changed_line_count: usize,
}

/// Builds a [`NonSymbolChange`] per `(path, changed_line_count)` pair,
/// sorted by `path` ascending — matching
/// [`crate::file_size::compute_file_size_bands`]'s ordering rationale:
/// there is no "most attention-worthy first" ranking here, just a
/// complete per-file listing in a stable order.
pub fn compute_non_symbol_changes(files: &[(String, usize)]) -> Vec<NonSymbolChange> {
    let mut entries: Vec<NonSymbolChange> = files
        .iter()
        .map(|(path, changed_line_count)| NonSymbolChange {
            path: path.clone(),
            changed_line_count: *changed_line_count,
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_empty_when_no_files_given() {
        let files: Vec<(String, usize)> = vec![];

        let expected: Vec<NonSymbolChange> = vec![];
        let actual = compute_non_symbol_changes(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_build_one_entry_per_file() {
        let files = vec![("src/index.ts".to_string(), 12)];

        let expected = vec![NonSymbolChange {
            path: "src/index.ts".to_string(),
            changed_line_count: 12,
        }];
        let actual = compute_non_symbol_changes(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_sort_entries_by_path_ascending() {
        let files = vec![
            ("z.py".to_string(), 3),
            ("a.py".to_string(), 5),
            ("m.py".to_string(), 1),
        ];

        let expected = vec![
            NonSymbolChange {
                path: "a.py".to_string(),
                changed_line_count: 5,
            },
            NonSymbolChange {
                path: "m.py".to_string(),
                changed_line_count: 1,
            },
            NonSymbolChange {
                path: "z.py".to_string(),
                changed_line_count: 3,
            },
        ];
        let actual = compute_non_symbol_changes(&files);

        assert_eq!(expected, actual);
    }
}
