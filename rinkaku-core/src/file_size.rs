//! File-size warnings and bands (ADR 0028, amended to add always-present
//! per-file bands): flag source files that have grown past empirical
//! readability thresholds, so a reviewer skimming rinkaku's output sees
//! oversized files at a glance rather than discovering the drift only when
//! a multi-thousand-line file finally needs a mechanical split.
//!
//! Pure over plain `(path, line_count)` pairs — [`crate::pipeline`]
//! collects the pairs while it already holds each file's content in scope
//! for parsing (skipped files — binary, generated, deleted,
//! unsupported-language — are excluded there, not here). This module then
//! classifies each pair into a [`FileSizeBand`] via [`classify_file_size`],
//! the single source of truth both [`compute_file_size_warnings`] (the
//! Warn/Split-only subset, historically the whole of this module) and
//! [`compute_file_size_bands`] (every file, all four bands) build on. Three
//! thresholds ([`NORMAL_LINE_THRESHOLD`], [`WARN_LINE_THRESHOLD`],
//! [`SPLIT_LINE_THRESHOLD`]) are fixed by ADR 0028 as part of the spec:
//! changing them is an ADR amendment, not a silent tune, so downstream
//! consumers (LLM reviewers, human review policy) can rely on the meaning
//! of each band staying stable.

use serde::Serialize;

/// Line-count threshold above which a file leaves [`FileSizeBand::Normal`]
/// and enters [`FileSizeBand::Watch`] (ADR 0028 amendment). Strictly
/// greater, same convention as [`WARN_LINE_THRESHOLD`] /
/// [`SPLIT_LINE_THRESHOLD`]. Descriptive only — unlike the other two
/// thresholds, no dedicated Markdown/JSON warning was ever built around
/// this boundary; it exists so [`FileSizeBand`] can classify every file,
/// not just the ones already worth a warning.
pub const NORMAL_LINE_THRESHOLD: usize = 600;

/// Line-count threshold above which a file is reported as
/// [`FileSizeSeverity::Warn`] / [`FileSizeBand::Warn`] (ADR 0028). The
/// check is strictly greater: a file at exactly [`WARN_LINE_THRESHOLD`]
/// lines is not warned about.
pub const WARN_LINE_THRESHOLD: usize = 1000;

/// Line-count threshold above which a file is reported as
/// [`FileSizeSeverity::Split`] / [`FileSizeBand::Split`] (ADR 0028).
/// Strictly greater, same as [`WARN_LINE_THRESHOLD`].
pub const SPLIT_LINE_THRESHOLD: usize = 1500;

/// Severity of a [`FileSizeWarning`] (ADR 0028): `Warn` = the file has
/// crossed the "start planning the split" watch threshold, `Split` = the
/// file has crossed the "this needs to be split, or the PR body must
/// justify the growth" threshold. Serializes as `"warn"` / `"split"` for
/// the JSON output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSizeSeverity {
    Warn,
    Split,
}

/// One file's oversized-file warning (ADR 0028), reported on
/// [`crate::render::Report::file_size_warnings`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileSizeWarning {
    pub path: String,
    pub line_count: usize,
    pub severity: FileSizeSeverity,
}

/// The four-tier line-count classification every analyzed file falls into
/// (ADR 0028 amendment), unlike [`FileSizeSeverity`] which only names the
/// two bands worth a dedicated warning. Serializes as `"normal"` /
/// `"watch"` / `"warn"` / `"split"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSizeBand {
    Normal,
    Watch,
    Warn,
    Split,
}

impl FileSizeBand {
    /// The [`FileSizeSeverity`] a [`FileSizeWarning`] would carry for this
    /// band, or `None` for [`FileSizeBand::Normal`] / [`FileSizeBand::Watch`]
    /// — bands below the warning thresholds have no `FileSizeSeverity`
    /// equivalent, since that type only exists to name the two bands worth
    /// a dedicated warning.
    pub fn severity(self) -> Option<FileSizeSeverity> {
        match self {
            FileSizeBand::Normal | FileSizeBand::Watch => None,
            FileSizeBand::Warn => Some(FileSizeSeverity::Warn),
            FileSizeBand::Split => Some(FileSizeSeverity::Split),
        }
    }
}

/// One file's line count and band (ADR 0028 amendment), reported on
/// [`crate::render::Report::file_size_bands`] for every analyzed file —
/// unlike [`FileSizeWarning`], which only covers the Warn/Split subset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileSizeEntry {
    pub path: String,
    pub line_count: usize,
    pub band: FileSizeBand,
}

/// Classifies a single `line_count` into a [`FileSizeBand`] (ADR 0028
/// amendment) — the one place the four threshold comparisons live, so
/// [`compute_file_size_warnings`] and [`compute_file_size_bands`] can't
/// drift apart on where a boundary falls.
pub fn classify_file_size(line_count: usize) -> FileSizeBand {
    if line_count > SPLIT_LINE_THRESHOLD {
        FileSizeBand::Split
    } else if line_count > WARN_LINE_THRESHOLD {
        FileSizeBand::Warn
    } else if line_count > NORMAL_LINE_THRESHOLD {
        FileSizeBand::Watch
    } else {
        FileSizeBand::Normal
    }
}

/// Computes the per-file size warnings for a `(path, line_count)` list,
/// following ADR 0028's thresholds and ordering:
///
/// - `line_count > SPLIT_LINE_THRESHOLD` -> [`FileSizeSeverity::Split`]
/// - `line_count > WARN_LINE_THRESHOLD` -> [`FileSizeSeverity::Warn`]
/// - anything else is dropped (not included in the returned vec)
///
/// The returned vec is sorted so the most attention-worthy entries come
/// first: severity descending (`Split` before `Warn`), then within one
/// severity, `line_count` descending, then `path` ascending for a stable
/// tiebreak. That order matches the Markdown surface's top-to-bottom
/// reading, so both the JSON consumer and the Markdown renderer share one
/// canonical ordering rather than each sorting on its own.
pub fn compute_file_size_warnings(files: &[(String, usize)]) -> Vec<FileSizeWarning> {
    let mut warnings: Vec<FileSizeWarning> = files
        .iter()
        .filter_map(|(path, line_count)| {
            let severity = classify_file_size(*line_count).severity()?;
            Some(FileSizeWarning {
                path: path.clone(),
                line_count: *line_count,
                severity,
            })
        })
        .collect();

    // Ordering rationale (matches ADR 0028's Markdown ordering):
    //   1. severity descending (Split before Warn) via `severity_rank`
    //   2. line_count descending
    //   3. path ascending (stable tiebreak)
    warnings.sort_by(|a, b| {
        severity_rank(b.severity)
            .cmp(&severity_rank(a.severity))
            .then_with(|| b.line_count.cmp(&a.line_count))
            .then_with(|| a.path.cmp(&b.path))
    });
    warnings
}

/// Computes every analyzed file's [`FileSizeEntry`] (ADR 0028 amendment),
/// unlike [`compute_file_size_warnings`] which drops everything at or
/// below [`WARN_LINE_THRESHOLD`]. Sorted by `path` ascending — there is no
/// "most attention-worthy first" ordering here the way
/// [`compute_file_size_warnings`] has, since the point of this function is
/// a complete per-file listing (the Markdown/TUI callers already know
/// which of their own rows are Watch/Warn/Split from the `band` field, and
/// render each file in their own existing order — this function does not
/// need to pre-sort attention).
pub fn compute_file_size_bands(files: &[(String, usize)]) -> Vec<FileSizeEntry> {
    let mut entries: Vec<FileSizeEntry> = files
        .iter()
        .map(|(path, line_count)| FileSizeEntry {
            path: path.clone(),
            line_count: *line_count,
            band: classify_file_size(*line_count),
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

/// Numeric weight used to sort [`FileSizeSeverity`] descending
/// (`Split` > `Warn`) without depending on the enum's Rust-side
/// `PartialOrd` derivation (which would leak into the public API).
fn severity_rank(severity: FileSizeSeverity) -> u8 {
    match severity {
        FileSizeSeverity::Split => 1,
        FileSizeSeverity::Warn => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_empty_when_no_files_are_over_threshold() {
        let files = vec![
            ("small.rs".to_string(), 10),
            ("medium.rs".to_string(), WARN_LINE_THRESHOLD),
            ("empty.rs".to_string(), 0),
        ];

        let expected: Vec<FileSizeWarning> = vec![];
        let actual = compute_file_size_warnings(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_report_warn_when_line_count_is_above_warn_threshold() {
        let files = vec![("watch.rs".to_string(), WARN_LINE_THRESHOLD + 1)];

        let expected = vec![FileSizeWarning {
            path: "watch.rs".to_string(),
            line_count: WARN_LINE_THRESHOLD + 1,
            severity: FileSizeSeverity::Warn,
        }];
        let actual = compute_file_size_warnings(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_report_split_when_line_count_is_above_split_threshold() {
        let files = vec![("huge.rs".to_string(), SPLIT_LINE_THRESHOLD + 1)];

        let expected = vec![FileSizeWarning {
            path: "huge.rs".to_string(),
            line_count: SPLIT_LINE_THRESHOLD + 1,
            severity: FileSizeSeverity::Split,
        }];
        let actual = compute_file_size_warnings(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_report_when_line_count_equals_warn_threshold() {
        // ADR 0028: the check is strictly greater. A file at exactly
        // WARN_LINE_THRESHOLD is not warned about; only WARN + 1 crosses.
        let files = vec![("edge.rs".to_string(), WARN_LINE_THRESHOLD)];

        let expected: Vec<FileSizeWarning> = vec![];
        let actual = compute_file_size_warnings(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_sort_split_before_warn() {
        // A `Warn` file with a higher line count than a `Split` file must
        // still sort after it: severity dominates line_count in the
        // ordering (ADR 0028).
        let files = vec![
            ("small_warn.rs".to_string(), WARN_LINE_THRESHOLD + 100),
            ("big_split.rs".to_string(), SPLIT_LINE_THRESHOLD + 1),
        ];

        let expected = vec![
            FileSizeWarning {
                path: "big_split.rs".to_string(),
                line_count: SPLIT_LINE_THRESHOLD + 1,
                severity: FileSizeSeverity::Split,
            },
            FileSizeWarning {
                path: "small_warn.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 100,
                severity: FileSizeSeverity::Warn,
            },
        ];
        let actual = compute_file_size_warnings(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_sort_by_line_count_desc_within_same_severity() {
        let files = vec![
            ("smaller.rs".to_string(), WARN_LINE_THRESHOLD + 10),
            ("bigger.rs".to_string(), WARN_LINE_THRESHOLD + 500),
            ("mid.rs".to_string(), WARN_LINE_THRESHOLD + 100),
        ];

        let expected = vec![
            FileSizeWarning {
                path: "bigger.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 500,
                severity: FileSizeSeverity::Warn,
            },
            FileSizeWarning {
                path: "mid.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 100,
                severity: FileSizeSeverity::Warn,
            },
            FileSizeWarning {
                path: "smaller.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 10,
                severity: FileSizeSeverity::Warn,
            },
        ];
        let actual = compute_file_size_warnings(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_sort_by_path_asc_when_line_count_and_severity_match() {
        let files = vec![
            ("z.rs".to_string(), WARN_LINE_THRESHOLD + 42),
            ("a.rs".to_string(), WARN_LINE_THRESHOLD + 42),
            ("m.rs".to_string(), WARN_LINE_THRESHOLD + 42),
        ];

        let expected = vec![
            FileSizeWarning {
                path: "a.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 42,
                severity: FileSizeSeverity::Warn,
            },
            FileSizeWarning {
                path: "m.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 42,
                severity: FileSizeSeverity::Warn,
            },
            FileSizeWarning {
                path: "z.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 42,
                severity: FileSizeSeverity::Warn,
            },
        ];
        let actual = compute_file_size_warnings(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_classify_normal_when_line_count_is_at_normal_threshold() {
        // Strictly-greater convention: exactly NORMAL_LINE_THRESHOLD stays
        // Normal, matching how WARN/SPLIT are strictly-greater too.
        let expected = FileSizeBand::Normal;
        let actual = classify_file_size(NORMAL_LINE_THRESHOLD);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_classify_watch_when_line_count_is_above_normal_threshold() {
        let expected = FileSizeBand::Watch;
        let actual = classify_file_size(NORMAL_LINE_THRESHOLD + 1);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_classify_watch_when_line_count_is_at_warn_threshold() {
        let expected = FileSizeBand::Watch;
        let actual = classify_file_size(WARN_LINE_THRESHOLD);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_classify_warn_when_line_count_is_above_warn_threshold() {
        let expected = FileSizeBand::Warn;
        let actual = classify_file_size(WARN_LINE_THRESHOLD + 1);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_classify_split_when_line_count_is_above_split_threshold() {
        let expected = FileSizeBand::Split;
        let actual = classify_file_size(SPLIT_LINE_THRESHOLD + 1);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_none_severity_when_band_is_normal_or_watch() {
        assert_eq!(None, FileSizeBand::Normal.severity());
        assert_eq!(None, FileSizeBand::Watch.severity());
    }

    #[test]
    fn should_return_matching_severity_when_band_is_warn_or_split() {
        assert_eq!(Some(FileSizeSeverity::Warn), FileSizeBand::Warn.severity());
        assert_eq!(
            Some(FileSizeSeverity::Split),
            FileSizeBand::Split.severity()
        );
    }

    #[test]
    fn should_include_every_file_when_computing_bands_regardless_of_band() {
        let files = vec![
            ("normal.rs".to_string(), 10),
            ("watch.rs".to_string(), NORMAL_LINE_THRESHOLD + 1),
            ("warn.rs".to_string(), WARN_LINE_THRESHOLD + 1),
            ("split.rs".to_string(), SPLIT_LINE_THRESHOLD + 1),
        ];

        let expected = vec![
            FileSizeEntry {
                path: "normal.rs".to_string(),
                line_count: 10,
                band: FileSizeBand::Normal,
            },
            FileSizeEntry {
                path: "split.rs".to_string(),
                line_count: SPLIT_LINE_THRESHOLD + 1,
                band: FileSizeBand::Split,
            },
            FileSizeEntry {
                path: "warn.rs".to_string(),
                line_count: WARN_LINE_THRESHOLD + 1,
                band: FileSizeBand::Warn,
            },
            FileSizeEntry {
                path: "watch.rs".to_string(),
                line_count: NORMAL_LINE_THRESHOLD + 1,
                band: FileSizeBand::Watch,
            },
        ];
        let actual = compute_file_size_bands(&files);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_when_computing_bands_over_an_empty_file_list() {
        let files: Vec<(String, usize)> = vec![];

        let expected: Vec<FileSizeEntry> = vec![];
        let actual = compute_file_size_bands(&files);

        assert_eq!(expected, actual);
    }
}
