//! Pipeline-result data shapes shared by every render format.
//!
//! Everything in this module is the plain-data half of the render layer:
//! [`Report`] (produced by `pipeline::analyze_diff` / `analyze_repo`) and
//! its component types plus [`SkipReason`]. `#[derive(Serialize)]` on all
//! of them is what `render`'s `OutputFormat::Json` branch serializes
//! directly — no hand-written JSON codegen lives here.

use crate::extract::{ExtractedSymbol, RemovedSymbol};
use crate::graph::{FanIn, SymbolGraph};
use serde::Serialize;

/// The result of running the extraction pipeline over a whole diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Which pipeline entry point produced this report (ADR 0017):
    /// [`ReportOrigin::Diff`] (the default — `analyze_diff`, every existing
    /// input mode) or [`ReportOrigin::RepoOutline`] (`analyze_repo`, the
    /// whole-repo default with no diff involved at all). Rendering reads
    /// this to pick change-oriented wording ("changed symbols") vs.
    /// outline-oriented wording ("symbols") for the same underlying data
    /// shape — see `render_markdown`'s "## Change graph"/"## Repository
    /// graph" split.
    ///
    /// `#[serde(default, skip_serializing_if = ...)]` keeps every existing
    /// `analyze_diff`-produced JSON report byte-for-byte unchanged: the
    /// field is omitted entirely when it's the default `Diff`, and only
    /// appears (as `"origin": "repo-outline"`) for the new whole-repo mode,
    /// which has no prior JSON shape to stay compatible with.
    #[serde(default, skip_serializing_if = "ReportOrigin::is_diff")]
    pub origin: ReportOrigin,
    pub files: Vec<FileReport>,
    pub skipped: Vec<SkippedFile>,
    /// The dependency graph over `files`' symbols (ADR 0008): edges and
    /// entry points used to render "Change graph" in Markdown, exposed here
    /// too so JSON consumers get the same structure without recomputing it.
    pub graph: SymbolGraph,
    /// Per-file counts of changed test symbols excluded from `files`
    /// under `--exclude-tests` (ADR 0009's mechanism; ADR 0025 flipped
    /// the default so this is now opt-in). Empty in the default run
    /// (test symbols stay in `files` like any other symbol) and only
    /// populated when the CLI passes `--exclude-tests`. Source order (the
    /// order files were first encountered in the diff), same as `files`.
    pub tests: Vec<TestFileSummary>,
    /// Fan-in symbols (ADR 0013, named "fan-in" per ADR 0034): changed
    /// symbols referenced by two or more other changed symbols, sorted by
    /// fan-in descending. Derived from `graph` via
    /// [`crate::graph::compute_fan_ins`] and kept as its own `Report` field
    /// (rather than recomputed at render time) so JSON consumers get it
    /// without recomputing the aggregation themselves, matching how `graph`
    /// itself is already exposed alongside `files`.
    pub fan_ins: Vec<FanIn>,
    /// File-size warnings (ADR 0028): source files whose line count crosses
    /// the [`crate::file_size::WARN_LINE_THRESHOLD`] / [`crate::file_size::SPLIT_LINE_THRESHOLD`]
    /// watch/split thresholds. Derived from the same per-file content
    /// [`crate::pipeline::analyze_diff`] and [`crate::pipeline::analyze_repo`]
    /// already read for parsing, via [`crate::file_size::compute_file_size_warnings`],
    /// and stored on `Report` (rather than recomputed at render time) so JSON
    /// consumers get it as an always-present top-level field, matching how
    /// `fan_ins` above is already exposed.
    pub file_size_warnings: Vec<crate::file_size::FileSizeWarning>,
    /// Every analyzed file's line count and [`crate::file_size::FileSizeBand`]
    /// (ADR 0028 amendment) — unlike `file_size_warnings` above, which only
    /// covers the Warn/Split subset, this covers every file so Markdown/TUI
    /// can show a line count next to every file, not only the ones already
    /// worth a dedicated warning. Derived from the same `(path, line_count)`
    /// pairs as `file_size_warnings`, via
    /// [`crate::file_size::compute_file_size_bands`], sorted by path
    /// ascending (see that function's doc comment for why).
    pub file_size_bands: Vec<crate::file_size::FileSizeEntry>,
    /// Symbols present on the base side of a diff but absent from the head
    /// side entirely (ADR 0014's `removed` classification) — reported
    /// separately from `files` since a removed symbol has no head-side
    /// signature/range/dependencies of its own. Always empty when no base
    /// content was available to classify against (see
    /// [`crate::pipeline::analyze_diff`]'s `read_base_file` parameter),
    /// same as every symbol's `classification` staying `None` in that case.
    pub removed: Vec<RemovedSymbol>,
}

/// Which pipeline entry point produced a [`Report`] (ADR 0017). `Default`
/// is `Diff` — every pre-ADR-0017 caller builds a `Report` via
/// `analyze_diff`, so defaulting to it is what keeps those `Report { ... }`
/// literals (and the JSON they serialize to) unchanged without having to
/// touch every one of them to spell out `origin` explicitly.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReportOrigin {
    #[default]
    Diff,
    RepoOutline,
}

impl ReportOrigin {
    /// Predicate form of `matches!(self, ReportOrigin::Diff)`, for
    /// `#[serde(skip_serializing_if = ...)]`, which needs a `fn(&T) -> bool`
    /// path rather than an inline expression.
    fn is_diff(&self) -> bool {
        matches!(self, ReportOrigin::Diff)
    }
}

/// Extracted symbols for a single changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileReport {
    pub path: String,
    pub symbols: Vec<ExtractedSymbol>,
}

/// How many changed test symbols were excluded from a given file's
/// `FileReport` (ADR 0009). Kept separate from `FileReport` rather than as
/// an extra field on it, since a file that is *entirely* tests (e.g. a Go
/// `*_test.go` file) would otherwise need an empty `FileReport` just to
/// carry this count — `tests` covers that file on its own instead.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestFileSummary {
    pub path: String,
    pub symbol_count: usize,
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
    /// `.gitattributes` marks this file `-diff` or `linguist-generated`
    /// (ADR 0010).
    Generated,
}

/// The short label shown for a [`SkipReason`] — `"unsupported language"`,
/// `"binary"`, `"deleted"`, `"generated"`. `pub` (rather than private to
/// this module) so other renderers of the same [`Report`] data — currently
/// `rinkaku-tui`'s entry-tree view — can show the identical wording instead
/// of maintaining a second copy of this match that could drift from
/// Markdown's.
pub fn skip_reason_label(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::UnsupportedLanguage => "unsupported language",
        SkipReason::Binary => "binary",
        SkipReason::Deleted => "deleted",
        SkipReason::Generated => "generated",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::{Classification, RemovedSymbol, SymbolKind};
    use crate::graph::Node;
    use crate::render::{OutputFormat, render};
    use pretty_assertions::assert_eq;

    /// Builds an `ExtractedSymbol` for rendering tests, with `id` set (the
    /// graph-building pipeline stage this module assumes already ran) and
    /// every other field defaulted to something inert unless overridden via
    /// struct-update syntax at the call site.
    fn symbol(id: &str, name: &str, kind: SymbolKind, signature: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            signature: signature.to_string(),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    fn node(id: &str, path: &str, name: &str) -> Node {
        Node {
            id: id.to_string(),
            path: path.to_string(),
            name: name.to_string(),
        }
    }

    // JSON is machine-readable output, not the human-skimmable Markdown
    // rendering — `Generated` entries must stay in `skipped` there for
    // full-fidelity consumers and so `garbage_input_note`'s "did we
    // recognize anything at all" check keeps working for an all-generated
    // diff (see `garbage_input_note_tests` in `rinkaku/src/main.rs`, which
    // reads `report.skipped` directly, not the rendered Markdown).
    #[test]
    fn should_keep_generated_entry_in_json_output() {
        let report = Report {
            origin: ReportOrigin::Diff,
            files: vec![],
            skipped: vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Generated,
            }],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        };

        let expected = "\
{
  \"files\": [],
  \"skipped\": [
    {
      \"path\": \"Cargo.lock\",
      \"reason\": \"generated\"
    }
  ],
  \"graph\": {
    \"nodes\": [],
    \"edges\": [],
    \"roots\": []
  },
  \"tests\": [],
  \"fan_ins\": [],
  \"file_size_warnings\": [],
  \"file_size_bands\": [],
  \"removed\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }

    // ADR 0017: `origin` must stay invisible in JSON for every existing
    // `analyze_diff`-produced report (see
    // `should_keep_generated_entry_in_json_output` above, whose expected
    // JSON has no `"origin"` key at all) — this is the flip side, pinning
    // that a whole-repo outline's `RepoOutline` origin *does* serialize, as
    // `"origin": "repo-outline"`, so JSON consumers can tell the two modes
    // apart.
    #[test]
    fn should_serialize_origin_field_when_report_is_a_repo_outline() {
        let report = Report {
            origin: ReportOrigin::RepoOutline,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        };

        let expected = "\
{
  \"origin\": \"repo-outline\",
  \"files\": [],
  \"skipped\": [],
  \"graph\": {
    \"nodes\": [],
    \"edges\": [],
    \"roots\": []
  },
  \"tests\": [],
  \"fan_ins\": [],
  \"file_size_warnings\": [],
  \"file_size_bands\": [],
  \"removed\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_json_with_graph_files_and_skipped_when_report_has_all_three() {
        let report = Report {
            origin: ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo()",
                )],
            }],
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            }],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        };

        let expected = "\
{
  \"files\": [
    {
      \"path\": \"src/lib.rs\",
      \"symbols\": [
        {
          \"id\": \"src/lib.rs::foo\",
          \"name\": \"foo\",
          \"kind\": \"Function\",
          \"signature\": \"fn foo()\",
          \"range\": {
            \"start\": 1,
            \"end\": 1
          },
          \"container\": null,
          \"dependencies\": [],
          \"omitted_matches\": 0
        }
      ]
    }
  ],
  \"skipped\": [
    {
      \"path\": \"assets/logo.png\",
      \"reason\": \"binary\"
    }
  ],
  \"graph\": {
    \"nodes\": [
      {
        \"id\": \"src/lib.rs::foo\",
        \"path\": \"src/lib.rs\",
        \"name\": \"foo\"
      }
    ],
    \"edges\": [],
    \"roots\": [
      \"src/lib.rs::foo\"
    ]
  },
  \"tests\": [],
  \"fan_ins\": [],
  \"file_size_warnings\": [],
  \"file_size_bands\": [],
  \"removed\": []
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_serialize_classification_and_previous_signature_and_removed_in_json() {
        let mut foo = symbol(
            "src/lib.rs::foo",
            "foo",
            SymbolKind::Function,
            "fn foo(a: i32, b: i32) -> i32",
        );
        foo.classification = Some(Classification::SignatureChanged);
        foo.previous_signature = Some("fn foo(a: i32) -> i32".to_string());
        let report = Report {
            origin: ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![foo],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
                edges: vec![],
                roots: vec!["src/lib.rs::foo".to_string()],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![RemovedSymbol {
                name: "old_helper".to_string(),
                kind: SymbolKind::Function,
                path: "src/lib.rs".to_string(),
                signature: "fn old_helper()".to_string(),
            }],
        };

        let expected = "\
{
  \"files\": [
    {
      \"path\": \"src/lib.rs\",
      \"symbols\": [
        {
          \"id\": \"src/lib.rs::foo\",
          \"name\": \"foo\",
          \"kind\": \"Function\",
          \"signature\": \"fn foo(a: i32, b: i32) -> i32\",
          \"range\": {
            \"start\": 1,
            \"end\": 1
          },
          \"container\": null,
          \"dependencies\": [],
          \"omitted_matches\": 0,
          \"classification\": \"signature_changed\",
          \"previous_signature\": \"fn foo(a: i32) -> i32\"
        }
      ]
    }
  ],
  \"skipped\": [],
  \"graph\": {
    \"nodes\": [
      {
        \"id\": \"src/lib.rs::foo\",
        \"path\": \"src/lib.rs\",
        \"name\": \"foo\"
      }
    ],
    \"edges\": [],
    \"roots\": [
      \"src/lib.rs::foo\"
    ]
  },
  \"tests\": [],
  \"fan_ins\": [],
  \"file_size_warnings\": [],
  \"file_size_bands\": [],
  \"removed\": [
    {
      \"name\": \"old_helper\",
      \"kind\": \"Function\",
      \"path\": \"src/lib.rs\",
      \"signature\": \"fn old_helper()\"
    }
  ]
}"
        .to_string();
        let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

        assert_eq!(expected, actual);
    }
}
