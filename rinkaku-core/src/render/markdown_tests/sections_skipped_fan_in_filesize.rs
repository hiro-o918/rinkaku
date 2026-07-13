//! The three optional post-graph sections — "## Skipped files",
//! "## High fan-in symbols" (ADR 0013, named per ADR 0034), and
//! "## File size warnings" (ADR 0028) — plus the ADR 0028 shape of
//! `file_size_warnings` in JSON output. Pins that each section is
//! emitted only when its list is non-empty and that they land in the
//! correct order relative to "Change graph" / "Definitions".

use super::*;
use crate::extract::SymbolKind;
use crate::graph::FanIn;
use crate::render::report::{FileReport, ReportOrigin, SkipReason, SkippedFile};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_render_skipped_files_section_when_report_has_skips() {
    let report = Report {
        origin: ReportOrigin::Diff,
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
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
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
fn should_render_change_graph_then_skipped_section_when_report_has_both() {
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

## Skipped files

- assets/logo.png (binary)
"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_omit_high_fan_in_symbols_section_when_fan_ins_is_empty() {
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
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
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

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_high_fan_in_symbols_section_between_change_graph_and_definitions_when_fan_ins_is_non_empty()
 {
    // `UpsertItemsRequest` (a struct) is referenced by two changed
    // functions — the label reuses tree_label's `{kind} {name}
    // ({path})` form, so the line reads
    // "struct UpsertItemsRequest (store/items.go) — used by 2: ..."
    // exactly as the ADR spec requires, and used_by names are joined
    // in the order `compute_fan_ins` already sorted them in (not
    // re-sorted here).
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "store/items.go".to_string(),
            symbols: vec![
                symbol(
                    "store/items.go::HandleFoo",
                    "HandleFoo",
                    SymbolKind::Function,
                    "func HandleFoo(req UpsertItemsRequest) error",
                ),
                symbol(
                    "store/items.go::HandleBar",
                    "HandleBar",
                    SymbolKind::Function,
                    "func HandleBar(req UpsertItemsRequest) error",
                ),
                symbol(
                    "store/items.go::UpsertItemsRequest",
                    "UpsertItemsRequest",
                    SymbolKind::Struct,
                    "type UpsertItemsRequest struct { Items []Item }",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("store/items.go::HandleFoo", "store/items.go", "HandleFoo"),
                node("store/items.go::HandleBar", "store/items.go", "HandleBar"),
                node(
                    "store/items.go::UpsertItemsRequest",
                    "store/items.go",
                    "UpsertItemsRequest",
                ),
            ],
            edges: vec![
                crate::graph::Edge {
                    from: "store/items.go::HandleFoo".to_string(),
                    to: "store/items.go::UpsertItemsRequest".to_string(),
                    is_cycle: false,
                },
                crate::graph::Edge {
                    from: "store/items.go::HandleBar".to_string(),
                    to: "store/items.go::UpsertItemsRequest".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec![
                "store/items.go::HandleFoo".to_string(),
                "store/items.go::HandleBar".to_string(),
            ],
        },
        tests: vec![],
        fan_ins: vec![FanIn {
            id: "store/items.go::UpsertItemsRequest".to_string(),
            path: "store/items.go".to_string(),
            name: "UpsertItemsRequest".to_string(),
            used_by: vec!["HandleBar".to_string(), "HandleFoo".to_string()],
        }],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

3 changed symbols in 1 file

- fn HandleFoo (store/items.go) — uses: UpsertItemsRequest
- fn HandleBar (store/items.go) — uses: UpsertItemsRequest

## High fan-in symbols

- struct UpsertItemsRequest (store/items.go) — used by 2: HandleBar, HandleFoo

## Definitions

### fn HandleFoo (store/items.go)

```
func HandleFoo(req UpsertItemsRequest) error
```

### struct UpsertItemsRequest (store/items.go)

```
type UpsertItemsRequest struct { Items []Item }
```

### fn HandleBar (store/items.go)

```
func HandleBar(req UpsertItemsRequest) error
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_fan_in_line_for_symbol_with_no_matching_definition() {
    // Same defensive rationale as the "NOTE" block below: `fan_ins`
    // could in principle reference a node id with no corresponding
    // `ExtractedSymbol` in `files` (the node is present in `graph`, so
    // the empty-output guard does not short-circuit, but `files` itself
    // has no matching symbol — mirroring the other lookup-miss tests'
    // setup). Unlike "Change graph"/"Definitions" (which use
    // `SymbolLookup`, keyed by symbol id, to find the container/
    // signature and skip the line entirely on a miss), the "High fan-in
    // symbols" line still renders on a lookup miss, falling back to a
    // bare `{name} ({path})` label with no kind prefix, rather than
    // being dropped outright.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::ghost", "src/lib.rs", "ghost")],
            edges: vec![],
            roots: vec!["src/lib.rs::ghost".to_string()],
        },
        tests: vec![],
        fan_ins: vec![FanIn {
            id: "src/lib.rs::ghost".to_string(),
            path: "src/lib.rs".to_string(),
            name: "ghost".to_string(),
            used_by: vec!["a".to_string(), "b".to_string()],
        }],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file


## High fan-in symbols

- ghost (src/lib.rs) — used by 2: a, b

## Definitions

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_file_size_warnings_section_when_warnings_are_present() {
    // Two warnings across both severities: the `Split` entry must come
    // first (ADR 0028 orders `Split` before `Warn`), each glyph and the
    // threshold-numbered explanation pinned exactly as the ADR spec
    // shows.
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
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![
            crate::file_size::FileSizeWarning {
                path: "b.rs".to_string(),
                line_count: 2500,
                severity: crate::file_size::FileSizeSeverity::Split,
            },
            crate::file_size::FileSizeWarning {
                path: "a.rs".to_string(),
                line_count: 1600,
                severity: crate::file_size::FileSizeSeverity::Warn,
            },
        ],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn foo (src/lib.rs)

## File size warnings

- 🚨 `b.rs` (2500 lines) — over the 2000-line split threshold
- ⚠ `a.rs` (1600 lines) — over the 1500-line watch threshold; consider splitting

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_omit_file_size_warnings_section_when_report_has_no_warnings() {
    // Empty `file_size_warnings` must drop the whole section — no bare
    // "## File size warnings" heading with nothing under it.
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
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert!(!actual.contains("## File size warnings"));
}

#[test]
fn should_place_file_size_warnings_between_high_fan_in_symbols_and_definitions() {
    // A report with both a high-fan-in symbol and a file-size warning:
    // the `## File size warnings` section must land after
    // `## High fan-in symbols` and before `## Definitions`, matching
    // ADR 0028's placement decision.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "store/items.go".to_string(),
            symbols: vec![
                symbol(
                    "store/items.go::HandleFoo",
                    "HandleFoo",
                    SymbolKind::Function,
                    "func HandleFoo(req UpsertItemsRequest) error",
                ),
                symbol(
                    "store/items.go::HandleBar",
                    "HandleBar",
                    SymbolKind::Function,
                    "func HandleBar(req UpsertItemsRequest) error",
                ),
                symbol(
                    "store/items.go::UpsertItemsRequest",
                    "UpsertItemsRequest",
                    SymbolKind::Struct,
                    "type UpsertItemsRequest struct { Items []Item }",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("store/items.go::HandleFoo", "store/items.go", "HandleFoo"),
                node("store/items.go::HandleBar", "store/items.go", "HandleBar"),
                node(
                    "store/items.go::UpsertItemsRequest",
                    "store/items.go",
                    "UpsertItemsRequest",
                ),
            ],
            edges: vec![
                crate::graph::Edge {
                    from: "store/items.go::HandleFoo".to_string(),
                    to: "store/items.go::UpsertItemsRequest".to_string(),
                    is_cycle: false,
                },
                crate::graph::Edge {
                    from: "store/items.go::HandleBar".to_string(),
                    to: "store/items.go::UpsertItemsRequest".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec![
                "store/items.go::HandleFoo".to_string(),
                "store/items.go::HandleBar".to_string(),
            ],
        },
        tests: vec![],
        fan_ins: vec![FanIn {
            id: "store/items.go::UpsertItemsRequest".to_string(),
            path: "store/items.go".to_string(),
            name: "UpsertItemsRequest".to_string(),
            used_by: vec!["HandleBar".to_string(), "HandleFoo".to_string()],
        }],
        file_size_warnings: vec![crate::file_size::FileSizeWarning {
            path: "store/items.go".to_string(),
            line_count: 2500,
            severity: crate::file_size::FileSizeSeverity::Split,
        }],
        removed: vec![],
    };

    let expected = "\
## Change graph

3 changed symbols in 1 file

- fn HandleFoo (store/items.go) — uses: UpsertItemsRequest
- fn HandleBar (store/items.go) — uses: UpsertItemsRequest

## High fan-in symbols

- struct UpsertItemsRequest (store/items.go) — used by 2: HandleBar, HandleFoo

## File size warnings

- 🚨 `store/items.go` (2500 lines) — over the 2000-line split threshold

## Definitions

### fn HandleFoo (store/items.go)

```
func HandleFoo(req UpsertItemsRequest) error
```

### struct UpsertItemsRequest (store/items.go)

```
type UpsertItemsRequest struct { Items []Item }
```

### fn HandleBar (store/items.go)

```
func HandleBar(req UpsertItemsRequest) error
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// ADR 0028: JSON output must always carry `file_size_warnings` as a
// top-level field, present-and-empty when nothing warns (mirroring how
// `fan_ins` is always present). This pins that shape end-to-end via
// the same `render` entry point the Markdown tests above use.
#[test]
fn should_include_file_size_warnings_in_json_output() {
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
        file_size_warnings: vec![
            crate::file_size::FileSizeWarning {
                path: "b.rs".to_string(),
                line_count: 2500,
                severity: crate::file_size::FileSizeSeverity::Split,
            },
            crate::file_size::FileSizeWarning {
                path: "a.rs".to_string(),
                line_count: 1600,
                severity: crate::file_size::FileSizeSeverity::Warn,
            },
        ],
        removed: vec![],
    };

    let expected = "\
{
  \"files\": [],
  \"skipped\": [],
  \"graph\": {
    \"nodes\": [],
    \"edges\": [],
    \"roots\": []
  },
  \"tests\": [],
  \"fan_ins\": [],
  \"file_size_warnings\": [
    {
      \"path\": \"b.rs\",
      \"line_count\": 2500,
      \"severity\": \"split\"
    },
    {
      \"path\": \"a.rs\",
      \"line_count\": 1600,
      \"severity\": \"warn\"
    }
  ],
  \"removed\": []
}"
    .to_string();
    let actual = render(&report, OutputFormat::Json).expect("json render succeeds");

    assert_eq!(expected, actual);
}
