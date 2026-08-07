//! ADR 0069: collapsing a `Depends on:` list into a `same as ... above`
//! reference when it exactly repeats an earlier symbol's list in the same
//! file.

use super::*;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::render::report::{FileReport, ReportOrigin};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_collapse_depends_on_when_identical_to_an_earlier_sibling() {
    let dependency = crate::deps::ResolvedSymbol {
        signature: "struct Point { x: i32 }".to_string(),
        path: "src/point.rs".to_string(),
        container: None,
    };
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    dependencies: vec![dependency.clone()],
                    ..symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                },
                ExtractedSymbol {
                    dependencies: vec![dependency.clone()],
                    ..symbol(
                        "src/lib.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        "fn bar(p: Point) -> i32",
                    )
                },
                ExtractedSymbol {
                    dependencies: vec![dependency],
                    ..symbol(
                        "src/lib.rs::baz",
                        "baz",
                        SymbolKind::Function,
                        "fn baz(p: Point) -> i32",
                    )
                },
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
                node("src/lib.rs::baz", "src/lib.rs", "baz"),
            ],
            edges: vec![],
            roots: vec![
                "src/lib.rs::foo".to_string(),
                "src/lib.rs::bar".to_string(),
                "src/lib.rs::baz".to_string(),
            ],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };

    let expected = "\
## Change graph

3 changed symbols in 1 file

- fn foo (src/lib.rs)
- fn bar (src/lib.rs)
- fn baz (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`

### fn bar (src/lib.rs)

```
fn bar(p: Point) -> i32
```

Depends on: same as `fn foo (src/lib.rs)` above

### fn baz (src/lib.rs)

```
fn baz(p: Point) -> i32
```

Depends on: same as `fn bar (src/lib.rs)` above

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_not_collapse_depends_on_when_lists_differ() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    dependencies: vec![crate::deps::ResolvedSymbol {
                        signature: "struct Point { x: i32 }".to_string(),
                        path: "src/point.rs".to_string(),
                        container: None,
                    }],
                    ..symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                },
                ExtractedSymbol {
                    dependencies: vec![crate::deps::ResolvedSymbol {
                        signature: "struct Line { a: Point, b: Point }".to_string(),
                        path: "src/line.rs".to_string(),
                        container: None,
                    }],
                    ..symbol(
                        "src/lib.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        "fn bar(l: Line) -> i32",
                    )
                },
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
            ],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };

    let expected = "\
## Change graph

2 changed symbols in 1 file

- fn foo (src/lib.rs)
- fn bar (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`

### fn bar (src/lib.rs)

```
fn bar(l: Line) -> i32
```

Depends on:
- `src/line.rs`: `struct Line { a: Point, b: Point }`

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_not_collapse_across_files() {
    let dependency = crate::deps::ResolvedSymbol {
        signature: "struct Point { x: i32 }".to_string(),
        path: "src/point.rs".to_string(),
        container: None,
    };
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/a.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    dependencies: vec![dependency.clone()],
                    ..symbol(
                        "src/a.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                }],
            },
            FileReport {
                path: "src/b.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    dependencies: vec![dependency],
                    ..symbol(
                        "src/b.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        "fn bar(p: Point) -> i32",
                    )
                }],
            },
        ],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/a.rs::foo", "src/a.rs", "foo"),
                node("src/b.rs::bar", "src/b.rs", "bar"),
            ],
            edges: vec![],
            roots: vec!["src/a.rs::foo".to_string(), "src/b.rs::bar".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };

    let expected = "\
## Change graph

2 changed symbols in 2 files — most in src/a.rs (1)

- fn foo (src/a.rs)
- fn bar (src/b.rs)

## Definitions

### fn foo (src/a.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`

### fn bar (src/b.rs)

```
fn bar(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

// The omitted-matches note is part of what "the same Depends on block"
// means (ADR 0069), so a differing note must prevent collapse even when
// the resolved `dependencies` list itself is identical.
#[test]
fn should_not_collapse_when_omitted_dependency_matches_differ() {
    let dependency = crate::deps::ResolvedSymbol {
        signature: "struct Point { x: i32 }".to_string(),
        path: "src/point.rs".to_string(),
        container: None,
    };
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    dependencies: vec![dependency.clone()],
                    omitted_dependency_matches: 1,
                    ..symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                },
                ExtractedSymbol {
                    dependencies: vec![dependency],
                    omitted_dependency_matches: 2,
                    ..symbol(
                        "src/lib.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        "fn bar(p: Point) -> i32",
                    )
                },
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
            ],
            edges: vec![],
            roots: vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };

    let expected = "\
## Change graph

2 changed symbols in 1 file

- fn foo (src/lib.rs)
- fn bar (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`
- (+1 more definitions matched by name)

### fn bar (src/lib.rs)

```
fn bar(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`
- (+2 more definitions matched by name)

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_not_collapse_against_an_older_match_when_a_differing_list_interposes() {
    // Pins the single-slot "nearest earlier match" tracker (ADR 0069):
    // an interposed differing list overwrites it, so an older identical
    // list is no longer a collapse target.
    let point = crate::deps::ResolvedSymbol {
        signature: "struct Point { x: i32 }".to_string(),
        path: "src/point.rs".to_string(),
        container: None,
    };
    let line = crate::deps::ResolvedSymbol {
        signature: "struct Line { a: Point, b: Point }".to_string(),
        path: "src/line.rs".to_string(),
        container: None,
    };
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                ExtractedSymbol {
                    dependencies: vec![point.clone()],
                    ..symbol(
                        "src/lib.rs::foo",
                        "foo",
                        SymbolKind::Function,
                        "fn foo(p: Point) -> i32",
                    )
                },
                ExtractedSymbol {
                    dependencies: vec![line],
                    ..symbol(
                        "src/lib.rs::bar",
                        "bar",
                        SymbolKind::Function,
                        "fn bar(l: Line) -> i32",
                    )
                },
                ExtractedSymbol {
                    dependencies: vec![point],
                    ..symbol(
                        "src/lib.rs::baz",
                        "baz",
                        SymbolKind::Function,
                        "fn baz(p: Point) -> i32",
                    )
                },
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
                node("src/lib.rs::baz", "src/lib.rs", "baz"),
            ],
            edges: vec![],
            roots: vec![
                "src/lib.rs::foo".to_string(),
                "src/lib.rs::bar".to_string(),
                "src/lib.rs::baz".to_string(),
            ],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };

    let expected = "\
## Change graph

3 changed symbols in 1 file

- fn foo (src/lib.rs)
- fn bar (src/lib.rs)
- fn baz (src/lib.rs)

## Definitions

### fn foo (src/lib.rs)

```
fn foo(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`

### fn bar (src/lib.rs)

```
fn bar(l: Line) -> i32
```

Depends on:
- `src/line.rs`: `struct Line { a: Point, b: Point }`

### fn baz (src/lib.rs)

```
fn baz(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32 }`

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
