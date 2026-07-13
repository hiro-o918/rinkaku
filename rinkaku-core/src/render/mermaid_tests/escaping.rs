//! Label escaping for quotes/brackets/ampersands and embedded newlines,
//! including its interaction with the ADR 0041 marker prefix.

use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_escape_label_when_name_contains_quote_and_bracket() {
    let report = empty_report(
        SymbolGraph {
            nodes: vec![node(
                "src/lib.rs::weird",
                "src/lib.rs",
                "weird\"name[with]brackets",
            )],
            edges: vec![],
            roots: vec!["src/lib.rs::weird".to_string()],
        },
        vec![],
    );

    let expected = format!(
        "flowchart LR\n  subgraph sub0[\"src/lib.rs\"]\n    n0[\"weird&quot;name&#91;with&#93;brackets\"]\n  end\n  class n0 referenced\n{}",
        CLASS_DEFS
    );
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_escape_label_when_added_node_name_contains_quote_and_bracket() {
    // No supported language's identifier grammar can produce these
    // characters via the CLI; pinned at the unit level via a direct
    // GraphNode construction instead.
    let report = empty_report(
        SymbolGraph {
            nodes: vec![node(
                "src/lib.rs::weird",
                "src/lib.rs",
                "weird\"name[with]brackets",
            )],
            edges: vec![],
            roots: vec!["src/lib.rs::weird".to_string()],
        },
        vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol(
                "src/lib.rs::weird",
                "weird\"name[with]brackets",
                SymbolKind::Function,
                Some(Classification::Added),
            )],
        }],
    );

    let expected = format!(
        "flowchart LR\n  subgraph sub0[\"src/lib.rs\"]\n    n0[\"+ weird&quot;name&#91;with&#93;brackets\"]\n  end\n  class n0 added\n{}",
        CLASS_DEFS
    );
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}

#[test]
fn should_replace_embedded_newline_with_space_when_path_contains_one() {
    // A path/name is not expected to legitimately contain a newline, but
    // nothing upstream guarantees it can't — an unescaped `\n` inside a
    // quoted mermaid label would break the single-line label syntax, so it
    // is normalized to a space defensively rather than left as-is or
    // escaped like the other special characters.
    let report = empty_report(
        SymbolGraph {
            nodes: vec![node("src/lib.rs::weird", "src/li\nb.rs", "weird")],
            edges: vec![],
            roots: vec!["src/lib.rs::weird".to_string()],
        },
        vec![],
    );

    let expected = format!(
        "flowchart LR\n  subgraph sub0[\"src/li b.rs\"]\n    n0[\"weird\"]\n  end\n  class n0 referenced\n{}",
        CLASS_DEFS
    );
    let actual = render(&report, OutputFormat::Mermaid).expect("mermaid render succeeds");

    assert_eq!(expected, actual);
}
