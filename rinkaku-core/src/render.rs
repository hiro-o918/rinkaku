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
use thiserror::Error;

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

/// Errors that can occur while rendering a [`Report`].
#[derive(Debug, Error)]
pub enum RenderError {
    /// Writing to the in-memory `String` buffer failed. This only happens
    /// on allocation failure, which `std::fmt::Write` reports as `Err(())`
    /// with no further detail; kept as a typed error (rather than
    /// `.unwrap()`) so the fallible write calls in `render_markdown` can
    /// use `?` instead of panicking.
    #[error("failed to write Markdown output")]
    Fmt(#[from] std::fmt::Error),
    #[error("failed to serialize report as JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Renders a [`Report`] in the requested [`OutputFormat`].
pub fn render(report: &Report, format: OutputFormat) -> Result<String, RenderError> {
    match format {
        OutputFormat::Markdown => render_markdown(report),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(report)?),
    }
}

/// Renders a [`Report`] as Markdown: one heading per file with its
/// symbols' signatures in a fenced code block, followed by a list of
/// skipped files.
fn render_markdown(report: &Report) -> Result<String, RenderError> {
    let mut out = String::new();

    for file in &report.files {
        writeln!(out, "## {}", file.path)?;
        writeln!(out)?;
        for symbol in &file.symbols {
            let container_line = symbol.container.as_deref().map(|c| format!("// {c}"));
            let fence = fence_for(container_line.as_deref(), &symbol.signature);
            writeln!(out, "{fence}")?;
            if let Some(container_line) = &container_line {
                writeln!(out, "{container_line}")?;
            }
            writeln!(out, "{}", symbol.signature)?;
            writeln!(out, "{fence}")?;
            writeln!(out)?;
        }
    }

    if !report.skipped.is_empty() {
        writeln!(out, "## Skipped files")?;
        writeln!(out)?;
        for skipped in &report.skipped {
            writeln!(
                out,
                "- {} ({})",
                skipped.path,
                skip_reason_label(skipped.reason)
            )?;
        }
    }

    Ok(out)
}

/// Picks a fence long enough that it cannot be closed early by a backtick
/// run inside the fenced content: one backtick longer than the longest run
/// of consecutive backticks in `content`, with a floor of 3 (the minimum
/// valid Markdown fence).
fn fence_for(container_line: Option<&str>, signature: &str) -> String {
    let longest_run = [container_line.unwrap_or(""), signature]
        .iter()
        .flat_map(|text| longest_backtick_run(text))
        .max()
        .unwrap_or(0);
    "`".repeat((longest_run + 1).max(3))
}

/// Length of the longest run of consecutive `` ` `` characters in `text`.
fn longest_backtick_run(text: &str) -> Option<usize> {
    text.split(|c| c != '`')
        .map(str::len)
        .filter(|&len| len > 0)
        .max()
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

    // Regression test: a signature containing a backtick code fence (e.g. a
    // doc comment example embedded in a macro invocation) used to break out
    // of the surrounding Markdown fence because it was always rendered with
    // exactly 3 backticks. The fence length must be at least one longer
    // than the longest run of backticks appearing in the rendered content.
    #[test]
    fn should_widen_fence_when_signature_contains_a_backtick_run() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    name: "example_macro".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn example_macro() { let s = \"```rust\\nfn f() {}\\n```\"; }"
                        .to_string(),
                    range: LineRange { start: 1, end: 1 },
                    container: None,
                }],
            }],
            skipped: vec![],
        };

        let expected = "\
## src/lib.rs

````
fn example_macro() { let s = \"```rust\\nfn f() {}\\n```\"; }
````

"
        .to_string();
        let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

        assert_eq!(expected, actual);
    }

    // Regression test: the container comment is part of the fenced block
    // too, so a backtick run inside the container name must also widen the
    // fence.
    #[test]
    fn should_widen_fence_when_container_contains_a_backtick_run() {
        let report = Report {
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    name: "bar".to_string(),
                    kind: SymbolKind::Function,
                    signature: "fn bar(&self) -> i32".to_string(),
                    range: LineRange { start: 4, end: 6 },
                    container: Some("impl Foo /* ```` */".to_string()),
                }],
            }],
            skipped: vec![],
        };

        let expected = "\
## src/lib.rs

`````
// impl Foo /* ```` */
fn bar(&self) -> i32
`````

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
