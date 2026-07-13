//! Empty-report short-circuit and section ordering: pins that
//! `render_markdown` emits nothing when there is nothing to say, and
//! that the "Tests" / "Other changed files" / "Skipped files"
//! sections appear in the documented order (and that `Generated`
//! skips are dropped from Markdown output entirely).

use super::*;
use crate::extract::SymbolKind;
use crate::render::report::{FileReport, ReportOrigin, SkipReason, SkippedFile};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_render_empty_markdown_when_report_has_no_files_and_no_skips() {
    let report = Report {
        origin: ReportOrigin::Diff,
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

    let expected = "".to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// Regression test: a pure rename (or mode-change-only diff) is reported
// as a `FileReport` with an empty `symbols` list (see
// `pipeline::analyze_diff`'s doc comment) rather than a `SkippedFile` —
// the file *was* looked at, it just had no symbol-level changes. Before
// this fix, such a file was silently dropped from Markdown output
// entirely (the empty-output guard fired because `graph.nodes` and
// `skipped` were both empty, even though `files` was not), which is a
// regression from the pre-ADR-0008 renderer that always emitted a `##
// {path}` heading for every entry in `report.files`.
#[test]
fn should_list_file_with_no_symbols_under_other_changed_files_when_report_has_no_graph_nodes() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/new_name.rs".to_string(),
            symbols: vec![],
        }],
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
## Other changed files

- src/new_name.rs

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_list_file_with_no_symbols_after_definitions_when_report_has_graph_nodes_too() {
    // A diff with one file that has a changed symbol (feeds the
    // "Change graph"/"Definitions" sections) alongside a pure-rename
    // file with no symbols at all — the pure rename must still show up,
    // in its own section after "Definitions".
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo()",
                )],
            },
            FileReport {
                path: "src/new_name.rs".to_string(),
                symbols: vec![],
            },
        ],
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
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

## Other changed files

- src/new_name.rs

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_other_changed_files_before_skipped_files_when_report_has_both() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/new_name.rs".to_string(),
            symbols: vec![],
        }],
        skipped: vec![SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: SkipReason::Binary,
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
## Other changed files

- src/new_name.rs

## Skipped files

- assets/logo.png (binary)
"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_tests_section_with_singular_symbol_noun_when_count_is_one() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![crate::render::TestFileSummary {
            path: "src/lib.rs".to_string(),
            symbol_count: 1,
        }],
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Tests

- src/lib.rs: 1 changed test symbol

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_tests_section_with_plural_symbols_noun_when_count_is_greater_than_one() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![crate::render::TestFileSummary {
            path: "src/lib.rs".to_string(),
            symbol_count: 3,
        }],
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Tests

- src/lib.rs: 3 changed test symbols

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_tests_section_between_definitions_and_other_changed_files_when_report_has_all_sections()
 {
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
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::foo", "src/lib.rs", "foo")],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string()],
        },
        tests: vec![crate::render::TestFileSummary {
            path: "src/lib.rs".to_string(),
            symbol_count: 2,
        }],
        fan_ins: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

## Tests

- src/lib.rs: 2 changed test symbols

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// Regression test: a `Generated` skip entry must not appear in Markdown
// output at all (not even under "Skipped files") — `.gitattributes`
// already marks these files as uninteresting to diff-review, so
// Markdown output (meant for humans/LLMs skimming a change) drops them
// silently rather than listing them as something rinkaku "didn't look
// at". They stay visible in JSON (see
// `should_keep_generated_entry_in_json_output` below) for machine
// consumers that want the full picture.
#[test]
fn should_omit_generated_skip_entry_from_markdown_output() {
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

    let expected = "".to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// Sibling case: when a `Generated` entry is mixed with other skip
// reasons, only the generated one is dropped from Markdown — the
// section itself still renders for the remaining, non-generated skips.
#[test]
fn should_omit_only_generated_entries_when_skipped_has_other_reasons_too() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![
            SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: SkipReason::Generated,
            },
            SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: SkipReason::Binary,
            },
        ],
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
## Skipped files

- assets/logo.png (binary)
"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
