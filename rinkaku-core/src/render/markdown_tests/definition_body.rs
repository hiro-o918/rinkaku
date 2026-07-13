//! `render_definition` behavior for a single symbol's "### ..." entry:
//! the container comment line, the `Depends on:` list (including the
//! omitted-matches note), and the fence-widening rules (`fence_for` /
//! `longest_backtick_run`) that keep backtick runs from escaping the
//! Markdown fence.

use super::*;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::render::report::{FileReport, ReportOrigin};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_render_container_comment_when_symbol_has_container() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                container: Some("impl Foo".to_string()),
                ..symbol(
                    "src/lib.rs::bar",
                    "bar",
                    SymbolKind::Function,
                    "fn bar(&self) -> i32",
                )
            }],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::bar", "src/lib.rs", "bar")],
            edges: vec![],
            roots: vec!["src/lib.rs::bar".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn bar (src/lib.rs)

## Definitions

### fn bar (src/lib.rs)

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
fn should_render_depends_on_list_when_symbol_has_dependencies() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                dependencies: vec![crate::deps::ResolvedSymbol {
                    signature: "struct Point { x: i32, y: i32 }".to_string(),
                    path: "src/point.rs".to_string(),
                }],
                ..symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo(p: Point) -> i32",
                )
            }],
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
fn foo(p: Point) -> i32
```

Depends on:
- `src/point.rs`: `struct Point { x: i32, y: i32 }`

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_multiple_depends_on_entries_when_symbol_has_several_dependencies() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                dependencies: vec![
                    crate::deps::ResolvedSymbol {
                        signature: "struct Point { x: i32 }".to_string(),
                        path: "src/a.rs".to_string(),
                    },
                    crate::deps::ResolvedSymbol {
                        signature: "struct Point { y: i32 }".to_string(),
                        path: "src/b.rs".to_string(),
                    },
                ],
                ..symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo(p: Point) -> i32",
                )
            }],
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
fn foo(p: Point) -> i32
```

Depends on:
- `src/a.rs`: `struct Point { x: i32 }`
- `src/b.rs`: `struct Point { y: i32 }`

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_omitted_matches_note_when_dependency_matches_were_capped() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                dependencies: vec![crate::deps::ResolvedSymbol {
                    signature: "struct Point { x: i32 }".to_string(),
                    path: "src/a.rs".to_string(),
                }],
                omitted_dependency_matches: 2,
                ..symbol(
                    "src/lib.rs::foo",
                    "foo",
                    SymbolKind::Function,
                    "fn foo(p: Point) -> i32",
                )
            }],
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
fn foo(p: Point) -> i32
```

Depends on:
- `src/a.rs`: `struct Point { x: i32 }`
- (+2 more definitions matched by name)

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
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::example_macro",
                "example_macro",
                SymbolKind::Function,
                "fn example_macro() { let s = \"```rust\\nfn f() {}\\n```\"; }",
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node(
                "src/lib.rs::example_macro",
                "src/lib.rs",
                "example_macro",
            )],
            edges: vec![],
            roots: vec!["src/lib.rs::example_macro".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn example_macro (src/lib.rs)

## Definitions

### fn example_macro (src/lib.rs)

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
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                container: Some("impl Foo /* ```` */".to_string()),
                ..symbol(
                    "src/lib.rs::bar",
                    "bar",
                    SymbolKind::Function,
                    "fn bar(&self) -> i32",
                )
            }],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/lib.rs::bar", "src/lib.rs", "bar")],
            edges: vec![],
            roots: vec!["src/lib.rs::bar".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        file_size_warnings: vec![],
        removed: vec![],
    };

    let expected = "\
## Change graph

1 changed symbol in 1 file

- fn bar (src/lib.rs)

## Definitions

### fn bar (src/lib.rs)

`````
// impl Foo /* ```` */
fn bar(&self) -> i32
`````

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
