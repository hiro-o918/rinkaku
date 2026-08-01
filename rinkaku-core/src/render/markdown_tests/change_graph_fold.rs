//! ADR 0012 decision 1's `— uses: ...` folding of childless
//! non-function children, including root exemption, per-parent
//! repetition, and the non-fold rule when a child has only cycle edges.

use super::*;
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::graph::Edge;
use crate::render::report::{FileReport, ReportOrigin};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_inline_two_leaf_struct_children_as_uses_annotation_on_method_line() {
    // A method referencing two childless, non-function structs (the
    // request/response shape the ADR calls out): both fold into the
    // parent's own line as `— uses: ...` instead of rendering as their
    // own nested lines, but both still get full "### ..." entries
    // under "Definitions" (ADR 0012 decision 1).
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "store/items.go".to_string(),
            symbols: vec![
                symbol(
                    "store/items.go::UpsertItems",
                    "UpsertItems",
                    SymbolKind::Function,
                    "func UpsertItems(req UpsertItemsRequest) (UpsertItemsResponse, error)",
                ),
                symbol(
                    "store/items.go::UpsertItemsRequest",
                    "UpsertItemsRequest",
                    SymbolKind::Struct,
                    "type UpsertItemsRequest struct { Items []Item }",
                ),
                symbol(
                    "store/items.go::UpsertItemsResponse",
                    "UpsertItemsResponse",
                    SymbolKind::Struct,
                    "type UpsertItemsResponse struct { Count int }",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node(
                    "store/items.go::UpsertItems",
                    "store/items.go",
                    "UpsertItems",
                ),
                node(
                    "store/items.go::UpsertItemsRequest",
                    "store/items.go",
                    "UpsertItemsRequest",
                ),
                node(
                    "store/items.go::UpsertItemsResponse",
                    "store/items.go",
                    "UpsertItemsResponse",
                ),
            ],
            edges: vec![
                Edge {
                    from: "store/items.go::UpsertItems".to_string(),
                    to: "store/items.go::UpsertItemsRequest".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "store/items.go::UpsertItems".to_string(),
                    to: "store/items.go::UpsertItemsResponse".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec!["store/items.go::UpsertItems".to_string()],
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

3 changed symbols in 1 file

- fn UpsertItems (store/items.go) — uses: UpsertItemsRequest, UpsertItemsResponse

## Definitions

### fn UpsertItems (store/items.go)

```
func UpsertItems(req UpsertItemsRequest) (UpsertItemsResponse, error)
```

### struct UpsertItemsRequest (store/items.go)

```
type UpsertItemsRequest struct { Items []Item }
```

### struct UpsertItemsResponse (store/items.go)

```
type UpsertItemsResponse struct { Count int }
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_disambiguate_folded_names_when_duplicate_symbols_fold_under_same_parent() {
    // Two distinct `Dup` structs in the same file (mirroring an
    // overloaded/shadowed-name scenario `graph::collect_nodes`
    // disambiguates by appending `@{start_line}` to the node id) both
    // fold under `foo`. Bare `Dup, Dup` would be ambiguous — Definitions
    // shows two distinct `### struct Dup (src/lib.rs:5)` /
    // `(src/lib.rs:10)` headers, so the folded annotation must use the
    // same `Name (path:line)` form `tree_label` already uses for
    // disambiguated symbols, not the bare name.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                ExtractedSymbol {
                    range: LineRange { start: 5, end: 6 },
                    ..symbol(
                        "src/lib.rs::Dup@5",
                        "Dup",
                        SymbolKind::Struct,
                        "struct Dup { a: i32 }",
                    )
                },
                ExtractedSymbol {
                    range: LineRange { start: 10, end: 11 },
                    ..symbol(
                        "src/lib.rs::Dup@10",
                        "Dup",
                        SymbolKind::Struct,
                        "struct Dup { b: i32 }",
                    )
                },
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::Dup@5", "src/lib.rs", "Dup"),
                node("src/lib.rs::Dup@10", "src/lib.rs", "Dup"),
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::Dup@5".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::Dup@10".to_string(),
                    is_cycle: false,
                },
            ],
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

3 changed symbols in 1 file

- fn foo (src/lib.rs) — uses: Dup (src/lib.rs:5), Dup (src/lib.rs:10)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Dup (src/lib.rs:5)

```
struct Dup { a: i32 }
```

### struct Dup (src/lib.rs:10)

```
struct Dup { b: i32 }
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_repeat_folded_struct_annotation_on_every_referencing_parent() {
    // Both `foo` and `bar` reference the same childless struct `Shared`
    // — unlike function children (which get a single full render plus
    // `(see above)` elsewhere, ADR 0008), a folded name has no
    // "see above" tracking: it legitimately repeats verbatim in the
    // `— uses: ...` annotation on every parent that references it, and
    // it must never itself get a `(see above)` line.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                symbol("src/lib.rs::bar", "bar", SymbolKind::Function, "fn bar()"),
                symbol(
                    "src/lib.rs::Shared",
                    "Shared",
                    SymbolKind::Struct,
                    "struct Shared { x: i32 }",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
                node("src/lib.rs::Shared", "src/lib.rs", "Shared"),
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::Shared".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::bar".to_string(),
                    to: "src/lib.rs::Shared".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec!["src/lib.rs::foo".to_string(), "src/lib.rs::bar".to_string()],
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

3 changed symbols in 1 file

- fn foo (src/lib.rs) — uses: Shared
- fn bar (src/lib.rs) — uses: Shared

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Shared (src/lib.rs)

```
struct Shared { x: i32 }
```

### fn bar (src/lib.rs)

```
fn bar()
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_fold_hcl_block_dependency_repeated_across_multiple_parents() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "main.tf".to_string(),
            symbols: vec![
                symbol(
                    "main.tf::aws_instance.web",
                    "aws_instance.web",
                    SymbolKind::Block,
                    "resource \"aws_instance\" \"web\" { ... }",
                ),
                symbol(
                    "main.tf::aws_instance.db",
                    "aws_instance.db",
                    SymbolKind::Block,
                    "resource \"aws_instance\" \"db\" { ... }",
                ),
                symbol(
                    "main.tf::var.region",
                    "var.region",
                    SymbolKind::Block,
                    "variable \"region\" { ... }",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("main.tf::aws_instance.web", "main.tf", "aws_instance.web"),
                node("main.tf::aws_instance.db", "main.tf", "aws_instance.db"),
                node("main.tf::var.region", "main.tf", "var.region"),
            ],
            edges: vec![
                Edge {
                    from: "main.tf::aws_instance.web".to_string(),
                    to: "main.tf::var.region".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "main.tf::aws_instance.db".to_string(),
                    to: "main.tf::var.region".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec![
                "main.tf::aws_instance.web".to_string(),
                "main.tf::aws_instance.db".to_string(),
            ],
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

3 changed symbols in 1 file

- block aws_instance.web (main.tf) — uses: var.region
- block aws_instance.db (main.tf) — uses: var.region

## Definitions

### block aws_instance.web (main.tf)

```
resource \"aws_instance\" \"web\" { ... }
```

### block var.region (main.tf)

```
variable \"region\" { ... }
```

### block aws_instance.db (main.tf)

```
resource \"aws_instance\" \"db\" { ... }
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_childless_non_function_root_as_top_level_line_when_it_would_otherwise_be_foldable()
{
    // `Config` is a childless struct — foldable by the structural
    // criterion — but it is also a root, so it must still render as
    // its own top-level tree line rather than being folded away
    // entirely (roots are always their own top-level DFS start).
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/config.rs".to_string(),
            symbols: vec![symbol(
                "src/config.rs::Config",
                "Config",
                SymbolKind::Struct,
                "struct Config { path: String }",
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node("src/config.rs::Config", "src/config.rs", "Config")],
            edges: vec![],
            roots: vec!["src/config.rs::Config".to_string()],
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

- struct Config (src/config.rs)

## Definitions

### struct Config (src/config.rs)

```
struct Config { path: String }
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_nested_line_when_non_function_child_has_its_own_children() {
    // `Wrapper` is a non-function child of `foo`, but it is not
    // foldable because it has an outgoing edge of its own (to `Inner`)
    // — the structural criterion is "childless", not "non-function",
    // so `Wrapper` itself still renders as a nested line exactly as
    // before this feature. `Inner`, in turn, *is* childless and
    // non-function, so it folds into `Wrapper`'s own line instead of
    // getting a third nesting level.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                symbol(
                    "src/lib.rs::Wrapper",
                    "Wrapper",
                    SymbolKind::Struct,
                    "struct Wrapper { inner: Inner }",
                ),
                symbol(
                    "src/lib.rs::Inner",
                    "Inner",
                    SymbolKind::Struct,
                    "struct Inner { x: i32 }",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::Wrapper", "src/lib.rs", "Wrapper"),
                node("src/lib.rs::Inner", "src/lib.rs", "Inner"),
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::Wrapper".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::Wrapper".to_string(),
                    to: "src/lib.rs::Inner".to_string(),
                    is_cycle: false,
                },
            ],
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

3 changed symbols in 1 file

- fn foo (src/lib.rs)
  - struct Wrapper (src/lib.rs) — uses: Inner

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Wrapper (src/lib.rs)

```
struct Wrapper { inner: Inner }
```

### struct Inner (src/lib.rs)

```
struct Inner { x: i32 }
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_not_fold_non_function_child_when_its_only_children_are_cycle_edges() {
    // `Node` is a non-function type whose only outgoing edge is a
    // cycle edge back to itself — `children_by_node` still records an
    // entry for it, so it is *not* foldable (folding requires no
    // outgoing edges at all) and must render as its own nested line
    // with the cycle warning still visible beneath it.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                symbol(
                    "src/lib.rs::Node",
                    "Node",
                    SymbolKind::Struct,
                    "struct Node { next: Option<Box<Node>> }",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::Node", "src/lib.rs", "Node"),
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::Node".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::Node".to_string(),
                    to: "src/lib.rs::Node".to_string(),
                    is_cycle: true,
                },
            ],
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

2 changed symbols in 1 file

- fn foo (src/lib.rs)
  - struct Node (src/lib.rs)
    - ⚠️ struct Node (src/lib.rs) — dependency cycle, see above

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### struct Node (src/lib.rs)

```
struct Node { next: Option<Box<Node>> }
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
