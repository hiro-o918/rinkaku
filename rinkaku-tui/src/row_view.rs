//! Pure formatting of one entry-view row into styled `ratatui` text
//! (stage B, ADR 0015/0016): given a `crate::nav::Row` (already resolved
//! against the tree/collapse state) plus its display label, produces the
//! [`ratatui::text::Line`] that `crate::ui` draws for it.
//!
//! `Line`/`Span`/`Style` are plain, `PartialEq`-comparable data — not
//! `Frame`/`Terminal` — so building one from a `Row` is a pure
//! transformation, unit-tested here the same way `crate::tree`/`crate::nav`
//! test their own plain-data outputs. Layout (which rows are visible at
//! all, where the split between panes falls, and computing each row's
//! `label` from ancestor context — see [`relative_label`]) is `crate::ui`'s
//! job; this module only decides one row's content and styling given that
//! label.

use crate::nav::Row;
use crate::order::DirRank;
use crate::tree::{Badges, NodeKind, SymbolRef};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use rinkaku_core::extract::{Classification, SymbolKind};
use std::collections::HashMap;

/// Indent width per tree depth level, in columns.
const INDENT_WIDTH: usize = 2;

/// Builds the styled [`Line`] for one visible row.
///
/// `label` is this row's display name: for a `Dir`/`File` row, the
/// segment(s) of `crate::tree::TreeNode::path` not already implied by an
/// ancestor row on screen (see [`relative_label`], which `crate::ui` uses
/// to compute it while walking the tree — a pure per-row function like
/// this one has no ancestor context of its own, since `crate::nav::Row`
/// only exposes one node). Unused for a `Symbol` row, which always
/// displays `SymbolRef::name` regardless of `label`.
///
/// `ranks` is consulted only for a [`NodeKind::Dir`] row, to show the
/// `(cycle)` warning marker (ADR 0016 decision 4 / ADR 0008's existing
/// symbol-level cycle warning) — a directory's own `crate::order::DirRank`
/// is looked up by its `path`, matching `crate::order`'s own map key.
/// `selected` styles the row for the cursor (reverse video), independent
/// of the node's own kind-based styling.
pub fn entry_row_line(
    row: &Row<'_>,
    label: &str,
    ranks: &HashMap<String, DirRank>,
    selected: bool,
) -> Line<'static> {
    let indent = " ".repeat(row.depth * INDENT_WIDTH);
    let mut spans = vec![Span::raw(indent)];

    match &row.node.kind {
        NodeKind::Dir => {
            let in_cycle = ranks.get(&row.node.path).is_some_and(|rank| rank.in_cycle);
            spans.push(Span::raw(format!("{} ", expand_marker(row))));
            spans.push(Span::styled(
                label.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(" "));
            spans.push(badges_span(&row.node.badges));
            if in_cycle {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    "(cycle)",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }
        NodeKind::File => {
            spans.push(Span::raw(format!("{} ", expand_marker(row))));
            spans.push(Span::raw(label.to_string()));
            spans.push(Span::raw(" "));
            spans.push(badges_span(&row.node.badges));
        }
        NodeKind::Symbol(symbol_ref) => {
            spans.push(Span::raw("  "));
            spans.push(symbol_marker_span(symbol_ref));
            spans.push(Span::raw(" "));
            spans.push(Span::raw(format!("{} ", kind_abbrev(symbol_ref.kind))));
            spans.push(Span::styled(
                symbol_ref.name.clone(),
                symbol_name_style(symbol_ref),
            ));
        }
    }

    let mut line = Line::from(spans);
    if selected {
        line = line.style(Style::default().add_modifier(Modifier::REVERSED));
    }
    line
}

/// The expand/collapse indicator for a `Dir`/`File` row: `" "` (blank) for
/// a childless node (nothing to expand), `"v"` when its children are
/// currently shown, `">"` when collapsed.
fn expand_marker(row: &Row<'_>) -> &'static str {
    if row.node.children.is_empty() {
        " "
    } else if row.expanded {
        "v"
    } else {
        ">"
    }
}

/// Computes the display label for every row in `rows` (as returned by
/// `crate::nav::Nav::rows`), stripping each `Dir`/`File` node's ancestor
/// directory path already shown by a preceding row on screen — e.g. a
/// `"src/foo"` directory nested under a `"src"` row displays as `"foo"`,
/// not the repeated `"src/foo"` its `TreeNode::path` carries in full (per
/// `crate::tree::TreeNode::path`'s own doc comment).
///
/// This only works correctly because `rows` is a pre-order flattening
/// (`Nav::rows`'s own doc comment): a directory's ancestor rows always
/// appear earlier in the slice, at a strictly smaller `depth`, so tracking
/// "the most recent path seen at each depth" while scanning forward once
/// is enough to compute every row's relative label in a single pass — no
/// need to reconstruct the tree's own recursive shape here. A `Symbol`
/// row's label is never consulted by `entry_row_line` (it always shows
/// `SymbolRef::name` instead), so this function returns an empty string
/// for those rows rather than computing a meaningless one.
pub fn relative_labels(rows: &[Row<'_>]) -> Vec<String> {
    // `ancestor_path_at[d]` is the full `TreeNode::path` of the most
    // recent `Dir`/`File` row seen at depth `d` — the nearest enclosing
    // ancestor for any later row at depth `d + 1`.
    let mut ancestor_path_at: Vec<Option<String>> = Vec::new();

    rows.iter()
        .map(|row| {
            if matches!(row.node.kind, NodeKind::Symbol(_)) {
                return String::new();
            }

            let parent_path = row.depth.checked_sub(1).and_then(|parent_depth| {
                ancestor_path_at
                    .get(parent_depth)
                    .and_then(|p| p.as_deref())
            });
            let label = match parent_path {
                Some(parent) => row
                    .node
                    .path
                    .strip_prefix(parent)
                    .and_then(|rest| rest.strip_prefix('/'))
                    .unwrap_or(&row.node.path)
                    .to_string(),
                None => row.node.path.clone(),
            };

            if ancestor_path_at.len() <= row.depth {
                ancestor_path_at.resize(row.depth + 1, None);
            }
            ancestor_path_at[row.depth] = Some(row.node.path.clone());
            // Truncate any deeper stale entries from a sibling subtree
            // this row's own depth just returned to (a pre-order walk can
            // go from a deep leaf back up to a shallower sibling), so a
            // later row never mistakes a previous, now-unrelated branch's
            // path for its actual parent.
            ancestor_path_at.truncate(row.depth + 1);

            label
        })
        .collect()
}

/// The compact badge summary shared by `Dir`/`File` rows: changed-symbol
/// count, contract-change count, and fan-in, each only shown when nonzero
/// (an all-zero badge set renders as an empty span, keeping quiet rows
/// quiet) — ASCII-only glyphs (`~`/`!`/`^`) chosen over Unicode/emoji for
/// terminal-compatibility (this implementation's own decision, see the
/// README's Interactive TUI section for the legend).
fn badges_span(badges: &Badges) -> Span<'static> {
    let mut parts = Vec::new();
    if badges.changed_symbols > 0 {
        parts.push(format!("~{}", badges.changed_symbols));
    }
    if badges.contract_changes > 0 {
        parts.push(format!("!{}", badges.contract_changes));
    }
    if badges.fan_in > 0 {
        parts.push(format!("^{}", badges.fan_in));
    }
    Span::styled(parts.join(" "), Style::default().fg(Color::Cyan))
}

/// A symbol row's leading classification marker: `+` added, `~`
/// signature-changed, ` ` (blank, one column) body-only or unclassified,
/// `x` removed. Kept as its own single-character span (rather than folded
/// into `symbol_name_style`) so it reads as a consistent left-aligned
/// column across rows regardless of name length.
fn symbol_marker_span(symbol_ref: &SymbolRef) -> Span<'static> {
    if symbol_ref.removed {
        return Span::styled("x", Style::default().fg(Color::Red));
    }
    match symbol_ref.classification {
        Some(Classification::Added) => Span::styled("+", Style::default().fg(Color::Green)),
        Some(Classification::SignatureChanged) => {
            Span::styled("~", Style::default().fg(Color::Yellow))
        }
        Some(Classification::BodyOnly) | None => Span::raw(" "),
    }
}

/// The symbol name's own style: a removed symbol renders dimmed +
/// crossed-out (`Modifier::CROSSED_OUT`, widely supported by modern
/// terminals) to read as "gone" at a glance, distinct from the marker span
/// above which only flags *why*.
fn symbol_name_style(symbol_ref: &SymbolRef) -> Style {
    if symbol_ref.removed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default()
    }
}

/// A short, fixed-width kind abbreviation for a symbol row (mirrors
/// `render.rs`'s Markdown rendering's own `fn`/`struct`/... prefixes, just
/// abbreviated further since the entry view is column-constrained).
fn kind_abbrev(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "fn",
        SymbolKind::Struct => "struct",
        SymbolKind::Enum => "enum",
        SymbolKind::Trait => "trait",
        SymbolKind::Class => "class",
        SymbolKind::Interface => "iface",
        SymbolKind::TypeAlias => "type",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{NodeKind, TreeNode};

    fn dir_node(path: &str, badges: Badges, children: Vec<TreeNode>) -> TreeNode {
        TreeNode {
            kind: NodeKind::Dir,
            path: path.to_string(),
            badges,
            children,
        }
    }

    fn file_node(path: &str, badges: Badges) -> TreeNode {
        TreeNode {
            kind: NodeKind::File,
            path: path.to_string(),
            badges,
            children: vec![],
        }
    }

    fn symbol_node(path: &str, symbol_ref: SymbolRef, badges: Badges) -> TreeNode {
        TreeNode {
            kind: NodeKind::Symbol(symbol_ref),
            path: path.to_string(),
            badges,
            children: vec![],
        }
    }

    fn plain_symbol(name: &str) -> SymbolRef {
        SymbolRef {
            id: format!("lib.rs::{name}"),
            name: name.to_string(),
            kind: SymbolKind::Function,
            classification: None,
            removed: false,
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn should_render_plain_text_for_zero_badges_and_no_classification() {
        let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
        let row = Row {
            node: &node,
            depth: 2,
            expanded: false,
        };

        let line = entry_row_line(&row, "", &HashMap::new(), false);

        assert_eq!("        fn foo", line_text(&line));
    }

    #[test]
    fn should_include_badge_glyphs_for_nonzero_badges_on_a_dir_row() {
        let node = dir_node(
            "src",
            Badges {
                changed_symbols: 2,
                contract_changes: 1,
                fan_in: 3,
            },
            vec![file_node("src/a.rs", Badges::default())],
        );
        let row = Row {
            node: &node,
            depth: 0,
            expanded: true,
        };

        let line = entry_row_line(&row, "src", &HashMap::new(), false);

        assert_eq!("v src ~2 !1 ^3", line_text(&line));
    }

    #[test]
    fn should_omit_zero_badges_entirely() {
        let node = file_node("lib.rs", Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

        assert_eq!("  lib.rs ", line_text(&line));
    }

    #[test]
    fn should_show_collapse_marker_when_dir_is_not_expanded() {
        let node = dir_node(
            "src",
            Badges::default(),
            vec![file_node("src/a.rs", Badges::default())],
        );
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "src", &HashMap::new(), false);

        assert_eq!("> src ", line_text(&line));
    }

    #[test]
    fn should_append_cycle_marker_when_dir_path_is_in_cycle() {
        let node = dir_node("src", Badges::default(), vec![]);
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };
        let mut ranks = HashMap::new();
        ranks.insert(
            "src".to_string(),
            DirRank {
                rank: 0,
                in_cycle: true,
            },
        );

        let line = entry_row_line(&row, "src", &ranks, false);

        assert_eq!("  src  (cycle)", line_text(&line));
    }

    #[test]
    fn should_not_append_cycle_marker_when_dir_path_is_not_in_cycle() {
        let node = dir_node("src", Badges::default(), vec![]);
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };
        let mut ranks = HashMap::new();
        ranks.insert(
            "src".to_string(),
            DirRank {
                rank: 0,
                in_cycle: false,
            },
        );

        let line = entry_row_line(&row, "src", &ranks, false);

        assert_eq!("  src ", line_text(&line));
    }

    #[test]
    fn should_mark_added_symbol_with_plus() {
        let symbol_ref = SymbolRef {
            classification: Some(Classification::Added),
            ..plain_symbol("new_fn")
        };
        let node = symbol_node("lib.rs", symbol_ref, Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "", &HashMap::new(), false);

        assert_eq!("  + fn new_fn", line_text(&line));
    }

    #[test]
    fn should_mark_signature_changed_symbol_with_tilde() {
        let symbol_ref = SymbolRef {
            classification: Some(Classification::SignatureChanged),
            ..plain_symbol("changed_fn")
        };
        let node = symbol_node("lib.rs", symbol_ref, Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "", &HashMap::new(), false);

        assert_eq!("  ~ fn changed_fn", line_text(&line));
    }

    #[test]
    fn should_mark_removed_symbol_with_x() {
        let symbol_ref = SymbolRef {
            removed: true,
            ..plain_symbol("gone_fn")
        };
        let node = symbol_node("lib.rs", symbol_ref, Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "", &HashMap::new(), false);

        assert_eq!("  x fn gone_fn", line_text(&line));
    }

    #[test]
    fn should_apply_reversed_modifier_when_row_is_selected() {
        let node = file_node("lib.rs", Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "lib.rs", &HashMap::new(), true);

        assert!(line.style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn should_not_apply_reversed_modifier_when_row_is_not_selected() {
        let node = file_node("lib.rs", Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

        assert!(!line.style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn should_indent_by_depth_times_indent_width() {
        let node = file_node("lib.rs", Badges::default());
        let row = Row {
            node: &node,
            depth: 3,
            expanded: false,
        };

        let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

        assert_eq!("        lib.rs ", line_text(&line));
    }

    #[test]
    fn should_strip_ancestor_prefix_from_nested_dir_label() {
        // "src" (depth 0) then "src/foo" (depth 1): the second row's label
        // must be just "foo", not the full "src/foo" its `path` carries.
        let root = dir_node("src", Badges::default(), vec![]);
        let child = dir_node("src/foo", Badges::default(), vec![]);
        let rows = vec![
            Row {
                node: &root,
                depth: 0,
                expanded: true,
            },
            Row {
                node: &child,
                depth: 1,
                expanded: false,
            },
        ];

        let labels = relative_labels(&rows);

        assert_eq!(vec!["src".to_string(), "foo".to_string()], labels);
    }

    #[test]
    fn should_keep_full_collapsed_label_when_no_ancestor_row_precedes_it() {
        // A root-level collapsed chain "src/foo/bar" has no ancestor row
        // above it at all, so its label stays the full collapsed path.
        let root = dir_node("src/foo/bar", Badges::default(), vec![]);
        let rows = vec![Row {
            node: &root,
            depth: 0,
            expanded: true,
        }];

        let labels = relative_labels(&rows);

        assert_eq!(vec!["src/foo/bar".to_string()], labels);
    }

    #[test]
    fn should_return_empty_label_for_symbol_rows() {
        let node = symbol_node("lib.rs", plain_symbol("foo"), Badges::default());
        let rows = vec![Row {
            node: &node,
            depth: 1,
            expanded: false,
        }];

        let labels = relative_labels(&rows);

        assert_eq!(vec![String::new()], labels);
    }

    #[test]
    fn should_recompute_label_correctly_after_returning_to_a_shallower_sibling() {
        // "a" (depth 0) -> "a/one.rs" (depth 1) -> "b" (depth 0, a sibling
        // of "a", not a descendant) -> "b/two.rs" (depth 1). The second
        // "depth 1" row must strip "b"'s prefix, not stale ancestor state
        // left over from "a"'s subtree.
        let a = dir_node("a", Badges::default(), vec![]);
        let a_file = file_node("a/one.rs", Badges::default());
        let b = dir_node("b", Badges::default(), vec![]);
        let b_file = file_node("b/two.rs", Badges::default());
        let rows = vec![
            Row {
                node: &a,
                depth: 0,
                expanded: true,
            },
            Row {
                node: &a_file,
                depth: 1,
                expanded: false,
            },
            Row {
                node: &b,
                depth: 0,
                expanded: true,
            },
            Row {
                node: &b_file,
                depth: 1,
                expanded: false,
            },
        ];

        let labels = relative_labels(&rows);

        assert_eq!(
            vec![
                "a".to_string(),
                "one.rs".to_string(),
                "b".to_string(),
                "two.rs".to_string(),
            ],
            labels
        );
    }
}
