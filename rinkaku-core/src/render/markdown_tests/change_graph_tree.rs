//! `render_change_graph` / `render_tree_node` behavior: nesting order,
//! true pre-order DFS, `(see above)` for multi-root-reached function
//! symbols, and cycle-warning rendering.

use super::*;
use crate::extract::SymbolKind;
use crate::graph::Edge;
use crate::render::report::{FileReport, ReportOrigin};
use crate::render::{OutputFormat, render};
use pretty_assertions::assert_eq;

#[test]
fn should_nest_callee_under_caller_in_change_graph_when_symbol_references_another() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/main.rs".to_string(),
            symbols: vec![
                symbol(
                    "src/main.rs::handle_pr",
                    "handle_pr",
                    SymbolKind::Function,
                    "fn handle_pr(args: PrArgs) -> Result<()>",
                ),
                symbol(
                    "src/main.rs::resolve_pr_base_sha",
                    "resolve_pr_base_sha",
                    SymbolKind::Function,
                    "fn resolve_pr_base_sha() -> Result<String>",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/main.rs::handle_pr", "src/main.rs", "handle_pr"),
                node(
                    "src/main.rs::resolve_pr_base_sha",
                    "src/main.rs",
                    "resolve_pr_base_sha",
                ),
            ],
            edges: vec![Edge {
                from: "src/main.rs::handle_pr".to_string(),
                to: "src/main.rs::resolve_pr_base_sha".to_string(),
                is_cycle: false,
            }],
            roots: vec!["src/main.rs::handle_pr".to_string()],
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

- fn handle_pr (src/main.rs)
  - fn resolve_pr_base_sha (src/main.rs)

## Definitions

### fn handle_pr (src/main.rs)

```
fn handle_pr(args: PrArgs) -> Result<()>
```

### fn resolve_pr_base_sha (src/main.rs)

```
fn resolve_pr_base_sha() -> Result<String>
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_order_definitions_in_true_pre_order_when_first_child_has_its_own_child() {
    // A -> B, A -> C (B before C in edge order), B -> D. True pre-order
    // DFS visits A, then descends fully into B's subtree (B, D) before
    // moving on to C: A, B, D, C. A naive "append to order when a node
    // is pushed onto the stack" (rather than when it is actually
    // visited/popped) would instead produce A, C, B, D, because C gets
    // pushed onto the stack right after A even though B is visited
    // first — this test pins the correct DFS order down as the full
    // rendered string so both the "Change graph" tree and "Definitions"
    // order are asserted together.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::a", "a", SymbolKind::Function, "fn a()"),
                symbol("src/lib.rs::b", "b", SymbolKind::Function, "fn b()"),
                symbol("src/lib.rs::c", "c", SymbolKind::Function, "fn c()"),
                symbol("src/lib.rs::d", "d", SymbolKind::Function, "fn d()"),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::a", "src/lib.rs", "a"),
                node("src/lib.rs::b", "src/lib.rs", "b"),
                node("src/lib.rs::c", "src/lib.rs", "c"),
                node("src/lib.rs::d", "src/lib.rs", "d"),
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::a".to_string(),
                    to: "src/lib.rs::b".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::a".to_string(),
                    to: "src/lib.rs::c".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::b".to_string(),
                    to: "src/lib.rs::d".to_string(),
                    is_cycle: false,
                },
            ],
            roots: vec!["src/lib.rs::a".to_string()],
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

4 changed symbols in 1 file

- fn a (src/lib.rs)
  - fn b (src/lib.rs)
    - fn d (src/lib.rs)
  - fn c (src/lib.rs)

## Definitions

### fn a (src/lib.rs)

```
fn a()
```

### fn b (src/lib.rs)

```
fn b()
```

### fn d (src/lib.rs)

```
fn d()
```

### fn c (src/lib.rs)

```
fn c()
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_mark_see_above_when_symbol_reachable_from_multiple_roots() {
    // Both "foo" and "bar" reference "shared": it must be rendered in
    // full once (under "foo", the first root in source order) and
    // referenced by name only under "bar" (ADR 0008).
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![
                symbol("src/lib.rs::foo", "foo", SymbolKind::Function, "fn foo()"),
                symbol("src/lib.rs::bar", "bar", SymbolKind::Function, "fn bar()"),
                symbol(
                    "src/lib.rs::shared",
                    "shared",
                    SymbolKind::Function,
                    "fn shared()",
                ),
            ],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/lib.rs::foo", "src/lib.rs", "foo"),
                node("src/lib.rs::bar", "src/lib.rs", "bar"),
                node("src/lib.rs::shared", "src/lib.rs", "shared"),
            ],
            edges: vec![
                Edge {
                    from: "src/lib.rs::foo".to_string(),
                    to: "src/lib.rs::shared".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/lib.rs::bar".to_string(),
                    to: "src/lib.rs::shared".to_string(),
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
        non_symbol_changes: vec![],
    };

    let expected = "\
## Change graph

3 changed symbols in 1 file

- fn foo (src/lib.rs)
  - fn shared (src/lib.rs)
- fn bar (src/lib.rs)
  - fn shared (src/lib.rs) (see above)

## Definitions

### fn foo (src/lib.rs)

```
fn foo()
```

### fn shared (src/lib.rs)

```
fn shared()
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
fn should_render_cycle_warning_when_edge_is_marked_as_cycle() {
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/git.rs".to_string(),
            symbols: vec![symbol(
                "src/git.rs::resolve_pr_base_sha",
                "resolve_pr_base_sha",
                SymbolKind::Function,
                "fn resolve_pr_base_sha()",
            )],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![node(
                "src/git.rs::resolve_pr_base_sha",
                "src/git.rs",
                "resolve_pr_base_sha",
            )],
            edges: vec![Edge {
                from: "src/git.rs::resolve_pr_base_sha".to_string(),
                to: "src/git.rs::resolve_pr_base_sha".to_string(),
                is_cycle: true,
            }],
            roots: vec!["src/git.rs::resolve_pr_base_sha".to_string()],
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

1 changed symbol in 1 file

- fn resolve_pr_base_sha (src/git.rs)
  - ⚠️ fn resolve_pr_base_sha (src/git.rs) — dependency cycle, see above

## Definitions

### fn resolve_pr_base_sha (src/git.rs)

```
fn resolve_pr_base_sha()
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_render_full_cycle_example_with_two_root_functions_and_a_dependency_cycle() {
    // The scenario from the ADR walkthrough: `handle_pr` calls
    // `resolve_pr_base_sha`, which calls `fetch_base_branch` and also
    // (a design smell the tool should surface) calls back into
    // itself. `Config` is an unrelated, independent root.
    let report = Report {
        origin: ReportOrigin::Diff,
        files: vec![
            FileReport {
                path: "src/main.rs".to_string(),
                symbols: vec![symbol(
                    "src/main.rs::handle_pr",
                    "handle_pr",
                    SymbolKind::Function,
                    "fn handle_pr(args: PrArgs) -> Result<()>",
                )],
            },
            FileReport {
                path: "src/git.rs".to_string(),
                symbols: vec![
                    symbol(
                        "src/git.rs::resolve_pr_base_sha",
                        "resolve_pr_base_sha",
                        SymbolKind::Function,
                        "fn resolve_pr_base_sha() -> Result<String>",
                    ),
                    symbol(
                        "src/git.rs::fetch_base_branch",
                        "fetch_base_branch",
                        SymbolKind::Function,
                        "fn fetch_base_branch() -> Result<()>",
                    ),
                ],
            },
            FileReport {
                path: "src/config.rs".to_string(),
                symbols: vec![symbol(
                    "src/config.rs::Config",
                    "Config",
                    SymbolKind::Struct,
                    "struct Config { path: String }",
                )],
            },
        ],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("src/main.rs::handle_pr", "src/main.rs", "handle_pr"),
                node(
                    "src/git.rs::resolve_pr_base_sha",
                    "src/git.rs",
                    "resolve_pr_base_sha",
                ),
                node(
                    "src/git.rs::fetch_base_branch",
                    "src/git.rs",
                    "fetch_base_branch",
                ),
                node("src/config.rs::Config", "src/config.rs", "Config"),
            ],
            edges: vec![
                Edge {
                    from: "src/main.rs::handle_pr".to_string(),
                    to: "src/git.rs::resolve_pr_base_sha".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/git.rs::resolve_pr_base_sha".to_string(),
                    to: "src/git.rs::fetch_base_branch".to_string(),
                    is_cycle: false,
                },
                Edge {
                    from: "src/git.rs::resolve_pr_base_sha".to_string(),
                    to: "src/git.rs::resolve_pr_base_sha".to_string(),
                    is_cycle: true,
                },
            ],
            roots: vec![
                "src/main.rs::handle_pr".to_string(),
                "src/config.rs::Config".to_string(),
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

4 changed symbols in 3 files — most in src/git.rs (2)

- fn handle_pr (src/main.rs)
  - fn resolve_pr_base_sha (src/git.rs)
    - fn fetch_base_branch (src/git.rs)
    - ⚠️ fn resolve_pr_base_sha (src/git.rs) — dependency cycle, see above
- struct Config (src/config.rs)

## Definitions

### fn handle_pr (src/main.rs)

```
fn handle_pr(args: PrArgs) -> Result<()>
```

### fn resolve_pr_base_sha (src/git.rs)

```
fn resolve_pr_base_sha() -> Result<String>
```

### fn fetch_base_branch (src/git.rs)

```
fn fetch_base_branch() -> Result<()>
```

### struct Config (src/config.rs)

```
struct Config { path: String }
```

"
    .to_string();
    let actual = render(&report, OutputFormat::Markdown).expect("markdown render succeeds");

    assert_eq!(expected, actual);
}
