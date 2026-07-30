//! "## Changes with no referencing tests" (ADR 0059): the fan-in mirror —
//! every changed, non-test symbol with `test_count == 0`, in the same
//! "omit when empty" and "sits between the other graph-derived sections"
//! shape `sections_skipped_fan_in_filesize` already pins for
//! "## High fan-in symbols".

use super::*;
use crate::extract::SymbolKind;
use crate::graph::TestCoverage;
use crate::render::report::{FileReport, ReportOrigin};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_omit_no_referencing_tests_section_when_test_coverage_is_empty() {
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
        test_coverage: vec![],
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

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_omit_no_referencing_tests_section_when_every_symbol_has_coverage() {
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
        test_coverage: vec![TestCoverage {
            id: "src/lib.rs::foo".to_string(),
            path: "src/lib.rs".to_string(),
            name: "foo".to_string(),
            covering_tests: vec!["src/lib.rs::spec_foo".to_string()],
            test_count: 1,
        }],
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

Tests: 1 (`src/lib.rs::spec_foo`)

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_no_referencing_tests_section_between_high_fan_in_symbols_and_file_sizes() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                symbol("src/lib.rs::a", "a", SymbolKind::Function, "fn a()"),
                symbol("src/lib.rs::b", "b", SymbolKind::Function, "fn b()"),
                symbol("src/lib.rs::bar", "bar", SymbolKind::Function, "fn bar()"),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::a", "src/lib.rs", "a"),
                node("src/lib.rs::b", "src/lib.rs", "b"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
            ],
            edges: vec![
                crate::graph::Edge {
                    from: "src/lib.rs::a".to_string(),
                    to: "src/lib.rs::bar".to_string(),
                    is_cycle: false,
                },
                crate::graph::Edge {
                    from: "src/lib.rs::b".to_string(),
                    to: "src/lib.rs::bar".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec![
                "src/lib.rs::foo".to_string(),
                "src/lib.rs::a".to_string(),
                "src/lib.rs::b".to_string(),
            ],
        },
        tests: vec![],
        fan_ins: vec![crate::graph::FanIn {
            id: "src/lib.rs::bar".to_string(),
            path: "src/lib.rs".to_string(),
            name: "bar".to_string(),
            used_by: vec!["a".to_string(), "b".to_string()],
        }],
        test_coverage: vec![
            TestCoverage {
                id: "src/lib.rs::foo".to_string(),
                path: "src/lib.rs".to_string(),
                name: "foo".to_string(),
                covering_tests: vec!["src/lib.rs::spec_foo".to_string()],
                test_count: 1,
            },
            TestCoverage {
                id: "src/lib.rs::bar".to_string(),
                path: "src/lib.rs".to_string(),
                name: "bar".to_string(),
                covering_tests: vec![],
                test_count: 0,
            },
        ],
        file_size_warnings: vec![],
        file_size_bands: vec![crate::file_size::FileSizeEntry {
            path: "src/lib.rs".to_string(),
            line_count: 10,
            band: crate::file_size::FileSizeBand::Normal,
        }],
        removed: vec![],
    };

    let expected = "\
## Change graph

4 changed symbols in 1 file

- fn foo (src/lib.rs)
- fn a (src/lib.rs)
  - fn bar (src/lib.rs)
- fn b (src/lib.rs)
  - fn bar (src/lib.rs) (see above)

## High fan-in symbols

- fn bar (src/lib.rs) — used by 2: a, b

## Changes with no referencing tests

- fn bar (src/lib.rs)

## File sizes

- `src/lib.rs` (10 lines)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

Tests: 1 (`src/lib.rs::spec_foo`)

### fn a (src/lib.rs)

```
fn a()
```

### fn bar (src/lib.rs)

```
fn bar()
```

Tests: 0

### fn b (src/lib.rs)

```
fn b()
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_repo_outline_heading_when_origin_is_repo_outline() {
    let report = Report {
        origin: ReportOrigin::RepoOutline,
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
        test_coverage: vec![TestCoverage {
            id: "src/lib.rs::foo".to_string(),
            path: "src/lib.rs".to_string(),
            name: "foo".to_string(),
            covering_tests: vec![],
            test_count: 0,
        }],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
    };

    let expected = "\
## Repository graph

1 symbol in 1 file

- fn foo (src/lib.rs)

## Symbols with no referencing tests

- fn foo (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

Tests: 0

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
