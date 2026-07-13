//! Blast-radius view-model (ADR 0019 for the re-rooting algorithm, ADR 0022
//! for the "blast radius" naming): given a directory or file path, builds
//! the entry-tree text the right pane shows when the user presses `R` on
//! that row — the interactive equivalent of `rinkaku --entry <path>`'s
//! Markdown "Change graph"/"Repository graph" section, flattened into plain
//! [`BlastRadiusLine`]s instead of Markdown text since the pane draws with
//! `ratatui`, not a Markdown renderer.
//!
//! [`build_blast_radius_view`] is a pure function of a [`Report`] and a
//! path: no IO, no `ratatui` types (mirrors `crate::detail`'s discipline).
//! It calls [`rinkaku_core::graph::pivot_graph`] once per invocation rather
//! than caching a re-rooted graph on `App` — matching ADR 0019's own
//! "recompute on toggle or cursor move while active, not per frame" stance
//! (ADR 0016's existing recompute-not-cache philosophy) and `crate::app`'s
//! wider convention of deriving view-models fresh from `Report` on each
//! call rather than threading derived state through `App`. That "not per
//! frame" half of the stance is enforced by the caller, not by this
//! function itself: `crate::run_app`'s event loop calls
//! [`crate::app::App::selected_blast_radius_view`] (which wraps this
//! function) at most once per handled key, caches the result, and hands the
//! cached value into `crate::ui::draw` — `crate::ui::draw_blast_radius_pane`
//! must not call either function itself, since `terminal.draw` also runs on
//! every idle poll tick, not only on a key press.
//!
//! `rinkaku-core`'s graph API (`pivot_graph`/`pivot_roots`) keeps its
//! existing name deliberately — ADR 0022 scopes the "blast radius" rename
//! to this crate's user-facing surface, not to `rinkaku-core` or the CLI's
//! `--entry` flag.

use rinkaku_core::extract::ExtractedSymbol;
use rinkaku_core::graph::{SymbolGraph, path_under_prefix, pivot_graph};
use rinkaku_core::render::Report;
use std::collections::{HashMap, HashSet};

/// One flattened line of the blast-radius tree: a node's label at a given
/// indentation depth, already carrying every display decision (whether it
/// is outside the pivoted prefix, already printed elsewhere, or a cycle
/// marker) so `crate::ui` only needs to lay the strings out, not re-derive
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastRadiusLine {
    pub depth: usize,
    /// `{kind} {name} ({path})`, matching `rinkaku-core::render`'s tree
    /// label shape so the blast-radius pane reads consistently with the
    /// Markdown "Change graph"/"Repository graph" section a reviewer may
    /// already be cross-referencing.
    pub label: String,
    /// `true` when this node's path falls outside the pivoted prefix
    /// (reached by expanding a dependency edge outward past the prefix
    /// boundary) — `crate::ui` dims these so the reviewer can tell "this is
    /// the layer I'm measuring from" from "this is what it reaches into".
    pub outside_prefix: bool,
    /// `true` when this node was already printed earlier in the tree; the
    /// line reads `{label} (see above)` and is not expanded further, same
    /// convention as `rinkaku-core::render`'s Markdown tree.
    pub already_printed: bool,
    /// `true` when this line is a cycle marker rather than a node line —
    /// `crate::ui` styles it distinctly (yellow/bold, mirroring the
    /// severity `rinkaku-core::render`'s Markdown `⚠️` warning signals),
    /// and `label` is the *target* node's label the cycle points back to.
    /// The label itself uses a plain `!` marker rather than `⚠️`
    /// deliberately (ADR 0022): `⚠️` is U+26A0 followed by a U+FE0F
    /// variation selector, and dynamic verification in a real terminal
    /// (`tmux capture-pane`) showed `unicode-width`'s single-column measurement
    /// of that pair disagreeing with the terminal's actual double-column
    /// rendering, desyncing `crate::ui::wrap_lines`'s column accounting from
    /// what the terminal draws and leaving a stray character on screen —
    /// exactly the risk ADR 0022 flagged before implementation.
    /// `rinkaku-core::render`'s Markdown output keeps `⚠️` unaffected: it
    /// is plain text there, never fed through a terminal-cell width
    /// calculation.
    pub is_cycle_warning: bool,
}

/// The blast-radius pane's content for a chosen path: either a tree of
/// [`BlastRadiusLine`]s, or `None` when no symbol's path falls under the
/// pivoted prefix at all (`crate::ui` shows a "no symbols under `<path>`"
/// style placeholder in that case, mirroring `main.rs`'s CLI-side
/// `entry_pivot_empty_note`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastRadiusView {
    pub path: String,
    pub lines: Vec<BlastRadiusLine>,
}

/// Builds the blast-radius view for `path` against `report`: re-roots
/// `report.graph` at `path` ([`pivot_graph`]) and flattens the resulting
/// tree into [`BlastRadiusLine`]s via a pre-order DFS from the pivoted
/// roots, mirroring `rinkaku-core::render::render_change_graph`'s walk
/// (same "(see above)" dedup, same cycle marker treatment) but without that
/// module's folding-into-parent-line behavior (ADR 0012 decision 1) — the
/// blast-radius pane is a compact secondary view, not the primary Markdown
/// output, and keeping every node on its own line is simpler to scan at a
/// glance in the pane's limited width.
///
/// Returns `None` when the pivoted graph has no roots at all: either no
/// node's path matched `path` (`pivot_graph` returns empty `roots` in that
/// case), or `report.graph` itself has no nodes to begin with.
pub fn build_blast_radius_view(report: &Report, path: &str) -> Option<BlastRadiusView> {
    let graph = pivot_graph(&report.graph, path);
    if graph.roots.is_empty() {
        return None;
    }

    let lookup = SymbolLookup::build(report);
    let children = children_by_node(&graph);
    let prefix_ids: HashSet<&str> = graph
        .nodes
        .iter()
        .filter(|node| path_under_prefix(&node.path, path))
        .map(|n| n.id.as_str())
        .collect();

    let mut lines = Vec::new();
    let mut printed: HashSet<String> = HashSet::new();
    for root in &graph.roots {
        walk(
            root,
            &children,
            &lookup,
            &prefix_ids,
            &mut printed,
            0,
            &mut lines,
        );
    }

    Some(BlastRadiusView {
        path: path.to_string(),
        lines,
    })
}

/// A symbol paired with the path of the file it lives in, keyed by node id
/// — mirrors `rinkaku_core::render`'s private `SymbolLookup`, rebuilt here
/// since that one is not exposed outside its module.
struct SymbolLookup<'a> {
    by_id: HashMap<&'a str, (&'a str, &'a ExtractedSymbol)>,
}

impl<'a> SymbolLookup<'a> {
    fn build(report: &'a Report) -> Self {
        let mut by_id = HashMap::new();
        for file in &report.files {
            for symbol in &file.symbols {
                by_id.insert(symbol.id.as_str(), (file.path.as_str(), symbol));
            }
        }
        Self { by_id }
    }

    fn get(&self, id: &str) -> Option<(&'a str, &'a ExtractedSymbol)> {
        self.by_id.get(id).copied()
    }

    fn label(&self, id: &str) -> Option<String> {
        self.get(id)
            .map(|(path, symbol)| format!("{} {} ({path})", kind_word(symbol.kind), symbol.name))
    }
}

fn kind_word(kind: rinkaku_core::extract::SymbolKind) -> &'static str {
    use rinkaku_core::extract::SymbolKind;
    match kind {
        SymbolKind::Function => "fn",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "interface",
        SymbolKind::TypeAlias => "type",
    }
}

/// Groups `graph.edges` by their `from` node, target annotated with whether
/// reaching it crosses a cycle edge — same shape as
/// `rinkaku-core::render`'s private `children_by_node`, rebuilt here for
/// the same reason `SymbolLookup` is.
fn children_by_node(graph: &SymbolGraph) -> HashMap<&str, Vec<(&str, bool)>> {
    let mut children: HashMap<&str, Vec<(&str, bool)>> = HashMap::new();
    for edge in &graph.edges {
        children
            .entry(edge.from.as_str())
            .or_default()
            .push((edge.to.as_str(), edge.is_cycle));
    }
    children
}

/// Recursive pre-order walk building [`BlastRadiusLine`]s, mirroring
/// `rinkaku-core::render::render_tree_node`'s dedup/cycle handling without
/// its inline-folding step (this module's own doc comment on why).
#[allow(clippy::too_many_arguments)]
fn walk(
    id: &str,
    children: &HashMap<&str, Vec<(&str, bool)>>,
    lookup: &SymbolLookup,
    prefix_ids: &HashSet<&str>,
    printed: &mut HashSet<String>,
    depth: usize,
    lines: &mut Vec<BlastRadiusLine>,
) {
    let Some(label) = lookup.label(id) else {
        return;
    };
    let outside_prefix = !prefix_ids.contains(id);

    if !printed.insert(id.to_string()) {
        lines.push(BlastRadiusLine {
            depth,
            label,
            outside_prefix,
            already_printed: true,
            is_cycle_warning: false,
        });
        return;
    }

    lines.push(BlastRadiusLine {
        depth,
        label,
        outside_prefix,
        already_printed: false,
        is_cycle_warning: false,
    });

    let kids = children.get(id).map(Vec::as_slice).unwrap_or(&[]);
    for &(child_id, is_cycle) in kids {
        if is_cycle {
            if let Some(child_label) = lookup.label(child_id) {
                lines.push(BlastRadiusLine {
                    depth: depth + 1,
                    label: format!("! {child_label} — already shown above (cycle)"),
                    outside_prefix: false,
                    already_printed: false,
                    is_cycle_warning: true,
                });
            }
            continue;
        }
        walk(
            child_id,
            children,
            lookup,
            prefix_ids,
            printed,
            depth + 1,
            lines,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::SymbolKind;
    use rinkaku_core::graph::{Edge, Node, SymbolGraph, build_graph};
    use rinkaku_core::render::FileReport;

    fn symbol(name: &str, referenced_names: Vec<&str>) -> ExtractedSymbol {
        ExtractedSymbol {
            id: String::new(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: referenced_names.into_iter().map(str::to_string).collect(),
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    fn empty_report() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        }
    }

    /// Builds a `Report` whose `graph`/`files`/symbol ids are all mutually
    /// consistent — `build_graph` assigns real ids and `stamp_ids` copies
    /// them back onto `files`, since `SymbolLookup`/`build_blast_radius_view` join
    /// the two by id, the same way `rinkaku-core::render`'s own tests do
    /// (`graph.rs`'s `should_stamp_each_symbol_id_...` fixtures).
    fn report_from(mut files: Vec<FileReport>) -> Report {
        let graph = build_graph(&files);
        rinkaku_core::graph::stamp_ids(&mut files, &graph);
        Report {
            files,
            graph,
            ..empty_report()
        }
    }

    #[test]
    fn should_return_none_when_no_symbol_matches_the_prefix() {
        let report = report_from(vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![symbol("foo", vec![])],
        }]);

        let actual = build_blast_radius_view(&report, "no/such/path");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_none_when_report_has_no_symbols_at_all() {
        let report = empty_report();

        let actual = build_blast_radius_view(&report, "src/api");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_build_single_line_when_prefix_matches_one_childless_symbol() {
        let report = report_from(vec![FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![symbol("api", vec![])],
        }]);

        let actual = build_blast_radius_view(&report, "src/api");

        let expected = BlastRadiusView {
            path: "src/api".to_string(),
            lines: vec![BlastRadiusLine {
                depth: 0,
                label: "fn api (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            }],
        };
        assert_eq!(Some(expected), actual);
    }

    #[test]
    fn should_mark_child_outside_prefix_when_dependency_lives_outside_the_pivoted_path() {
        // "api" (under src/api) depends on "helper" (under src/util) — the
        // tree still expands outward through the whole graph (ADR 0019),
        // but "helper"'s line must be flagged `outside_prefix: true` so the
        // pane can dim it.
        let report = report_from(vec![
            FileReport {
                path: "src/api/handler.rs".to_string(),
                symbols: vec![symbol("api", vec!["helper"])],
            },
            FileReport {
                path: "src/util.rs".to_string(),
                symbols: vec![symbol("helper", vec![])],
            },
        ]);

        let actual = build_blast_radius_view(&report, "src/api").expect("blast radius view");

        let expected_lines = vec![
            BlastRadiusLine {
                depth: 0,
                label: "fn api (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
            BlastRadiusLine {
                depth: 1,
                label: "fn helper (src/util.rs)".to_string(),
                outside_prefix: true,
                already_printed: false,
                is_cycle_warning: false,
            },
        ];
        assert_eq!(expected_lines, actual.lines);
    }

    #[test]
    fn should_mark_repeated_node_as_see_above_when_reachable_from_two_parents() {
        // "shared" is reachable from both "foo" and "bar", both under
        // src/api (a diamond, per graph.rs's own
        // `should_find_multiple_roots_when_two_independent_entry_points_exist`
        // fixture) — the second occurrence must be a `(see above)`-style
        // reference (`already_printed: true`), not expanded again.
        let report = report_from(vec![FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![
                symbol("foo", vec!["shared"]),
                symbol("bar", vec!["shared"]),
                symbol("shared", vec![]),
            ],
        }]);

        let actual = build_blast_radius_view(&report, "src/api").expect("blast radius view");

        let expected_lines = vec![
            BlastRadiusLine {
                depth: 0,
                label: "fn foo (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
            BlastRadiusLine {
                depth: 1,
                label: "fn shared (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
            BlastRadiusLine {
                depth: 0,
                label: "fn bar (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
            BlastRadiusLine {
                depth: 1,
                label: "fn shared (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: true,
                is_cycle_warning: false,
            },
        ];
        assert_eq!(expected_lines, actual.lines);
    }

    #[test]
    fn should_mark_cycle_edge_as_warning_line_when_pivot_root_participates_in_a_cycle() {
        // foo -> bar -> foo under src/api: pivoting at src/api re-roots at
        // "foo" (the sole SCC representative), and the back edge bar -> foo
        // must render as a cycle marker line, not be walked into again.
        let report = report_from(vec![FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec!["foo"])],
        }]);

        let actual = build_blast_radius_view(&report, "src/api").expect("blast radius view");

        let expected_lines = vec![
            BlastRadiusLine {
                depth: 0,
                label: "fn foo (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
            BlastRadiusLine {
                depth: 1,
                label: "fn bar (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
            BlastRadiusLine {
                depth: 2,
                label: "! fn foo (src/api/handler.rs) — already shown above (cycle)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: true,
            },
        ];
        assert_eq!(expected_lines, actual.lines);
    }

    #[test]
    fn should_visit_every_pivot_root_in_order_when_multiple_roots_exist() {
        let report = report_from(vec![FileReport {
            path: "src/api/handler.rs".to_string(),
            symbols: vec![symbol("foo", vec![]), symbol("bar", vec![])],
        }]);

        let actual = build_blast_radius_view(&report, "src/api").expect("blast radius view");

        let expected_lines = vec![
            BlastRadiusLine {
                depth: 0,
                label: "fn foo (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
            BlastRadiusLine {
                depth: 0,
                label: "fn bar (src/api/handler.rs)".to_string(),
                outside_prefix: false,
                already_printed: false,
                is_cycle_warning: false,
            },
        ];
        assert_eq!(expected_lines, actual.lines);
    }

    #[test]
    fn should_return_none_when_hand_built_graph_has_no_nodes_under_prefix() {
        let report = Report {
            graph: SymbolGraph {
                nodes: vec![Node {
                    id: "lib.rs::foo".to_string(),
                    path: "lib.rs".to_string(),
                    name: "foo".to_string(),
                }],
                edges: vec![Edge {
                    from: "lib.rs::foo".to_string(),
                    to: "lib.rs::foo".to_string(),
                    is_cycle: false,
                }],
                roots: vec!["lib.rs::foo".to_string()],
            },
            ..empty_report()
        };

        let actual = build_blast_radius_view(&report, "other");

        assert_eq!(None, actual);
    }
}
