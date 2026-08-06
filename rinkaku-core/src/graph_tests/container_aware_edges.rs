//! Tests for `collect_edges`'s container-matching rule (ADR 0068): a bare
//! `referenced_names` entry may only match a changed symbol with no
//! container or the same container as the referencing symbol, while a
//! `referenced_method_names` entry matches any container, same as every
//! reference matched before this ADR.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_not_build_edge_when_bare_reference_matches_a_same_named_symbol_in_a_different_container()
{
    // Reproduces the bug ADR 0068 fixes: a bare `Foo()` call (Python/Go/
    // TypeScript's grammars never capture a member-access call as a bare
    // reference) must not link to a `Foo` nested inside an unrelated
    // container just because the names collide.
    let referrer = symbol("use_foo", vec!["Foo"]);
    let top_level_foo = symbol("Foo", vec![]);
    let contained_foo = ExtractedSymbol {
        container: Some("class Baz".to_string()),
        range: LineRange { start: 10, end: 10 },
        ..symbol("Foo", vec![])
    };
    let files = vec![
        FileReport {
            path: "b.py".to_string(),
            symbols: vec![referrer],
        },
        FileReport {
            path: "a.py".to_string(),
            symbols: vec![top_level_foo, contained_foo],
        },
    ];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "b.py::use_foo".to_string(),
                path: "b.py".to_string(),
                name: "use_foo".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "a.py::Foo@1".to_string(),
                path: "a.py".to_string(),
                name: "Foo".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "a.py::Foo@10".to_string(),
                path: "a.py".to_string(),
                name: "Foo".to_string(),
                container: Some("class Baz".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "b.py::use_foo".to_string(),
            to: "a.py::Foo@1".to_string(),
            is_cycle: false,
        }],
        roots: vec!["b.py::use_foo".to_string(), "a.py::Foo@10".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_bare_reference_matches_a_top_level_symbol() {
    let files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("caller", vec!["helper"]), symbol("helper", vec![])],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "src/lib.rs::caller".to_string(),
                path: "src/lib.rs".to_string(),
                name: "caller".to_string(),
                container: None,
                is_test: false,
            },
            Node {
                id: "src/lib.rs::helper".to_string(),
                path: "src/lib.rs".to_string(),
                name: "helper".to_string(),
                container: None,
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "src/lib.rs::caller".to_string(),
            to: "src/lib.rs::helper".to_string(),
            is_cycle: false,
        }],
        roots: vec!["src/lib.rs::caller".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_bare_reference_matches_a_symbol_in_the_same_container() {
    let referrer = ExtractedSymbol {
        container: Some("class Point".to_string()),
        ..symbol("area", vec!["helper"])
    };
    let target = ExtractedSymbol {
        container: Some("class Point".to_string()),
        ..symbol("helper", vec![])
    };
    let files = vec![FileReport {
        path: "shapes.py".to_string(),
        symbols: vec![referrer, target],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "shapes.py::area".to_string(),
                path: "shapes.py".to_string(),
                name: "area".to_string(),
                container: Some("class Point".to_string()),
                is_test: false,
            },
            Node {
                id: "shapes.py::helper".to_string(),
                path: "shapes.py".to_string(),
                name: "helper".to_string(),
                container: Some("class Point".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "shapes.py::area".to_string(),
            to: "shapes.py::helper".to_string(),
            is_cycle: false,
        }],
        roots: vec!["shapes.py::area".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}

#[test]
fn should_build_edge_when_method_reference_matches_a_symbol_in_a_different_container() {
    // A `referenced_method_names` entry (Rust's `x.foo()`, trait method
    // names, Go/TypeScript interface method specs) may legitimately
    // denote a symbol in any container, so it keeps the unrestricted
    // matching every reference had before ADR 0068.
    let referrer = ExtractedSymbol {
        container: Some("fn main".to_string()),
        referenced_method_names: vec!["save".to_string()],
        ..symbol("run", vec![])
    };
    let target = ExtractedSymbol {
        container: Some("impl Repo".to_string()),
        ..symbol("save", vec![])
    };
    let files = vec![FileReport {
        path: "repo.rs".to_string(),
        symbols: vec![referrer, target],
    }];

    let expected = SymbolGraph {
        nodes: vec![
            Node {
                id: "repo.rs::run".to_string(),
                path: "repo.rs".to_string(),
                name: "run".to_string(),
                container: Some("fn main".to_string()),
                is_test: false,
            },
            Node {
                id: "repo.rs::save".to_string(),
                path: "repo.rs".to_string(),
                name: "save".to_string(),
                container: Some("impl Repo".to_string()),
                is_test: false,
            },
        ],
        edges: vec![Edge {
            from: "repo.rs::run".to_string(),
            to: "repo.rs::save".to_string(),
            is_cycle: false,
        }],
        roots: vec!["repo.rs::run".to_string()],
    };
    let actual = build_graph(&files);

    assert_eq!(expected, actual);
}
