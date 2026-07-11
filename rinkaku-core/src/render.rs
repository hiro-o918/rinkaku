//! Rendering the extraction pipeline's results into an output format.
//!
//! [`Report`] is the pipeline-wide result shape produced by
//! [`crate::pipeline::analyze_diff`]: per-file extracted symbols plus the
//! files that were skipped (unsupported language, binary, or deleted).
//! This module turns a `Report` into either Markdown (the default, meant
//! for humans and LLMs) or JSON (`serde`-derived, for machine consumption).
//!
//! Skipped files are always listed, never silently dropped — a reviewer
//! or LLM consuming the output needs to know what rinkaku didn't look at.

use crate::extract::ExtractedSymbol;
use serde::Serialize;
use std::fmt::Write as _;

/// The result of running the extraction pipeline over a whole diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub files: Vec<FileReport>,
    pub skipped: Vec<SkippedFile>,
}

/// Extracted symbols for a single changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileReport {
    pub path: String,
    pub symbols: Vec<ExtractedSymbol>,
}

/// A file the pipeline did not extract symbols from, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    pub path: String,
    pub reason: SkipReason,
}

/// Why a changed file was skipped rather than analyzed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// No registered [`crate::language::LanguageSupport`] for this file's
    /// extension.
    UnsupportedLanguage,
    /// Git reported this as a binary file patch.
    Binary,
    /// The file was deleted; there is no new-side content to extract from.
    Deleted,
}

/// Supported output formats for a [`Report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Markdown,
    Json,
}

/// Renders a [`Report`] in the requested [`OutputFormat`].
///
/// JSON rendering only fails if `serde_json` itself fails to serialize the
/// `Report`, which does not happen for this data shape (no maps with
/// non-string keys, no floats); the `Result` is kept so callers don't need
/// to special-case a `.unwrap()` at the boundary.
pub fn render(report: &Report, format: OutputFormat) -> Result<String, serde_json::Error> {
    match format {
        OutputFormat::Markdown => Ok(render_markdown(report)),
        OutputFormat::Json => serde_json::to_string_pretty(report),
    }
}

/// Renders a [`Report`] as Markdown: one heading per file with its
/// symbols' signatures in a fenced code block, followed by a list of
/// skipped files.
fn render_markdown(report: &Report) -> String {
    let mut out = String::new();

    for file in &report.files {
        writeln!(out, "## {}", file.path).unwrap();
        writeln!(out).unwrap();
        for symbol in &file.symbols {
            writeln!(out, "```").unwrap();
            if let Some(container) = &symbol.container {
                writeln!(out, "// {container}").unwrap();
            }
            writeln!(out, "{}", symbol.signature).unwrap();
            writeln!(out, "```").unwrap();
            writeln!(out).unwrap();
        }
    }

    if !report.skipped.is_empty() {
        writeln!(out, "## Skipped files").unwrap();
        writeln!(out).unwrap();
        for skipped in &report.skipped {
            writeln!(
                out,
                "- {} ({})",
                skipped.path,
                skip_reason_label(skipped.reason)
            )
            .unwrap();
        }
    }

    out
}

fn skip_reason_label(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::UnsupportedLanguage => "unsupported language",
        SkipReason::Binary => "binary",
        SkipReason::Deleted => "deleted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::SymbolKind;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_render_empty_markdown_when_report_has_no_files_and_no_skips() {
        let report = Report {
            files: vec![],
            skipped: vec![],
        };

        let expected = "".to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_markdown_heading_and_fenced_signature_when_file_has_one_symbol() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn foo(a: i32) -> i32".to_string(),
                    range: LineRange { start: 1, end: 3 },
                    container: None,
                }],
            }],
            skipped: vec![],
        };

        let expected = "\
## src/lib.rs

```
fn foo(a: i32) -> i32
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_container_comment_when_symbol_has_container() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    name: "bar".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn bar(&self) -> i32".to_string(),
                    range: LineRange { start: 4, end: 6 },
                    container: Some("impl Foo".to_string()),
                }],
            }],
            skipped: vec![],
        };

        let expected = "\
## src/lib.rs

```
// impl Foo
fn bar(&self) -> i32
```

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_skipped_files_section_when_report_has_skips() {
        let report = Report {
            files: vec![],
            skipped: vec![
                SkippedFile {
                    path: "assets/logo.png".to_string(),
                    reason: SkipReason::Binary,
                },
                SkippedFile {
                    path: "src/main.py".to_string(),
                    reason: SkipReason::UnsupportedLanguage,
                },
                SkippedFile {
                    path: "src/old.rs".to_string(),
                    reason: SkipReason::Deleted,
                },
            ],
        };

        let expected = "\
## Skipped files

- assets/logo.png (binary)
- src/main.py (unsupported language)
- src/old.rs (deleted)
"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_files_then_skipped_section_when_report_has_both() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn foo()".to_string(),
                    range: LineRange { start: 1, end: 1 },
                    container: None,
                }],
            }],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
        };

        let expected = "\
## src/lib.rs

```
fn foo()
```

## Skipped files

- assets/logo.png (binary)
"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_json_with_files_and_skipped_when_report_has_both() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    name: "foo".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn foo()".to_string(),
                    range: LineRange { start: 1, end: 1 },
                    container: None,
                }],
            }],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
        };

        let expected = "\
{
  \"files\": [
    {
      \"path\": \"src/lib.rs\",
      \"symbols\": [
        {
          \"name\": \"foo\",
          \"kind\": \"Function\",
          \"signature\": \"fn foo()\",
          \"range\": {
            \"start\": 1,
            \"end\": 1
          },
          \"container\": null
        }
      ]
    }
  ],
  \"skipped\": [
    {
      \"path\": \"assets/logo.png\",
      \"reason\": \"binary\"
    }
  ]
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }
}
