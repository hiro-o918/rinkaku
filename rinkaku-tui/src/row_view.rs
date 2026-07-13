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
use rinkaku_core::file_size::FileSizeSeverity;
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
            push_badge_spans(&mut spans, &row.node.badges, BadgeContext::Dir);
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
            spans.push(Span::styled(label.to_string(), file_label_style(row.node)));
            spans.push(Span::raw(" "));
            push_badge_spans(&mut spans, &row.node.badges, BadgeContext::File);
            // `skip_reason` is mutually exclusive with `test_symbol_count`
            // (`crate::tree::TreeNode::skip_reason`'s own doc comment: a
            // skipped file has no `FileReport`/`TestFileSummary` entry of
            // its own), so this `if`/`else if` only ever needs to choose
            // between "skipped" and "has a test count" — but `test_symbol_count`
            // is *not* exclusive with real `symbols`/badges shown above (a
            // mixed file legitimately has both, `TreeNode::test_symbol_count`'s
            // own doc comment), so the test badge still renders alongside the
            // ordinary changed-symbol badges for such a row rather than being
            // hidden by them. The priority (skip reason first, when present)
            // matches `crate::ui::file_detail_lines`'s own skip-reason-first
            // early return, so the two panes never disagree about which
            // explanation wins.
            if let Some(reason) = row.node.skip_reason {
                spans.push(Span::raw(" "));
                spans.push(skip_reason_span(reason));
            } else if let Some(count) = row.node.test_symbol_count {
                spans.push(Span::raw(" "));
                spans.push(test_badge_span(count));
            }
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

/// Which side of the tree a badge row is on — `Dir` rows render the
/// aggregated file-size warning counts (`warn:N split:N`), `File` rows
/// render this file's own `lines:N` instead. `NodeKind::Symbol` never
/// reaches badge rendering (symbol rows have their own layout, no badge
/// summary), so this only needs the two cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BadgeContext {
    Dir,
    File,
}

/// Appends the compact badge summary for a `Dir`/`File` row to `spans`:
/// changed-symbol count, contract-change count, fan-in, and (per
/// `context`) either the file-size-warning aggregate (`warn:N split:N`
/// on a directory) or this file's own line count (`lines:N` on a file
/// row). Each badge is only emitted when its underlying counter is
/// nonzero (an all-zero badge set adds nothing, keeping quiet rows
/// quiet).
///
/// Badge encoding rationale:
/// - The changed-symbol and fan-in badges use text labels
///   (`chg:` / `ref:`) matching the file-size badges' `lines:` / `warn:`
///   / `split:` convention (see ADR 0013 amendment 2026-07-13). The
///   single-glyph prefixes they replaced (`~` for changed, `^` for
///   fan-in) conveyed no semantic hint on their own to a first-time
///   reviewer. Only the numeric N picks up cyan; the label stays
///   default so the eye lands on the number, matching the file-size
///   badges' split-span pattern.
/// - `!{N}` (contract-change count) keeps its compact `!` glyph — the
///   ADR 0013 amendment that added `chg:`/`ref:` explicitly scopes the
///   rename to `~` and `^`, so `!N` is left untouched.
/// - The file-size warnings (ADR 0028) deliberately use **text labels
///   plus color** rather than an emoji glyph (`⚠` / `🚨`): terminal
///   emoji rendering width is inconsistent enough to distort the tree
///   column layout, and the color already encodes severity. Only the
///   numeric N portion is colored (yellow for Warn, red for Split); the
///   `lines:` / `warn:` / `split:` label stays default so the eye lands
///   on the number.
fn push_badge_spans(spans: &mut Vec<Span<'static>>, badges: &Badges, context: BadgeContext) {
    let cyan = Style::default().fg(Color::Cyan);
    let mut wrote_any_ascii_badge = false;
    if badges.changed_symbols > 0 {
        spans.push(Span::raw("chg:"));
        spans.push(Span::styled(badges.changed_symbols.to_string(), cyan));
        wrote_any_ascii_badge = true;
    }
    if badges.contract_changes > 0 {
        if wrote_any_ascii_badge {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("!{}", badges.contract_changes), cyan));
        wrote_any_ascii_badge = true;
    }
    if badges.fan_in > 0 {
        if wrote_any_ascii_badge {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::raw("ref:"));
        spans.push(Span::styled(badges.fan_in.to_string(), cyan));
        wrote_any_ascii_badge = true;
    }

    // File-size warning badges (ADR 0028) — text label + color, no
    // emoji. Rendered as separate spans so only the numeric N picks up
    // the severity color.
    match context {
        BadgeContext::File => {
            if let (Some(severity), Some(line_count)) =
                (badges.own_file_size_severity, badges.own_file_line_count)
            {
                // Leading space only when a preceding badge wrote
                // something — otherwise the row would gain a stray gap.
                if wrote_any_ascii_badge {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::raw("lines:"));
                spans.push(Span::styled(
                    line_count.to_string(),
                    Style::default().fg(severity_color(severity)),
                ));
            }
        }
        BadgeContext::Dir => {
            let has_warn = badges.file_size_warn_count > 0;
            let has_split = badges.file_size_split_count > 0;
            if (has_warn || has_split) && wrote_any_ascii_badge {
                spans.push(Span::raw(" "));
            }
            if has_warn {
                spans.push(Span::raw("warn:"));
                spans.push(Span::styled(
                    badges.file_size_warn_count.to_string(),
                    Style::default().fg(Color::Yellow),
                ));
            }
            if has_warn && has_split {
                spans.push(Span::raw(" "));
            }
            if has_split {
                spans.push(Span::raw("split:"));
                spans.push(Span::styled(
                    badges.file_size_split_count.to_string(),
                    Style::default().fg(Color::Red),
                ));
            }
        }
    }
}

/// Maps a [`FileSizeSeverity`] to its display color — yellow for `Warn`,
/// red for `Split`. Shared by the file-row `lines:N` badge and the
/// directory-row `warn:N` / `split:N` aggregates so both surfaces agree
/// on the color legend.
fn severity_color(severity: FileSizeSeverity) -> Color {
    match severity {
        FileSizeSeverity::Warn => Color::Yellow,
        FileSizeSeverity::Split => Color::Red,
    }
}

/// A `File` row's label style: dimmed for a skipped file (nothing was
/// extracted from it, so it reads visually as "less relevant" than an
/// analyzed file, same intent as `symbol_name_style`'s dimming of a removed
/// symbol), plain otherwise — including a whole-test-file row, which is
/// still an ordinarily-styled label with its own `[test]` badge appended
/// separately (see `test_badge_span`) rather than dimmed, since a test file
/// is not "uninteresting", just excluded from the default symbol-level view
/// (ADR 0009).
fn file_label_style(node: &crate::tree::TreeNode) -> Style {
    if node.skip_reason.is_some() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    }
}

/// The `(skipped: <reason>)` annotation for a skipped `File` row, reusing
/// [`rinkaku_core::render::skip_reason_label`]'s exact wording so the TUI
/// and Markdown output never describe the same `SkipReason` differently.
fn skip_reason_span(reason: rinkaku_core::render::SkipReason) -> Span<'static> {
    Span::styled(
        format!(
            "(skipped: {})",
            rinkaku_core::render::skip_reason_label(reason)
        ),
        Style::default().fg(Color::DarkGray),
    )
}

/// The `[test] (N symbols)` badge for a whole-test-file `File` row (a file
/// with no `FileReport` in `report.files` at all, see
/// `crate::tree::TreeNode::test_symbol_count`'s doc comment) — `N symbol`
/// (singular) when there is exactly one, matching `render.rs`'s own
/// singular/plural "Tests" section wording.
fn test_badge_span(symbol_count: usize) -> Span<'static> {
    let noun = if symbol_count == 1 {
        "symbol"
    } else {
        "symbols"
    };
    Span::styled(
        format!("[test] ({symbol_count} {noun})"),
        Style::default().fg(Color::Magenta),
    )
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
            skip_reason: None,
            test_symbol_count: None,
        }
    }

    fn file_node(path: &str, badges: Badges) -> TreeNode {
        TreeNode {
            kind: NodeKind::File,
            path: path.to_string(),
            badges,
            children: vec![],
            skip_reason: None,
            test_symbol_count: None,
        }
    }

    fn skipped_file_node(path: &str, reason: rinkaku_core::render::SkipReason) -> TreeNode {
        TreeNode {
            kind: NodeKind::File,
            path: path.to_string(),
            badges: Badges::default(),
            children: vec![],
            skip_reason: Some(reason),
            test_symbol_count: None,
        }
    }

    fn test_file_node(path: &str, symbol_count: usize) -> TreeNode {
        TreeNode {
            kind: NodeKind::File,
            path: path.to_string(),
            badges: Badges::default(),
            children: vec![],
            skip_reason: None,
            test_symbol_count: Some(symbol_count),
        }
    }

    fn symbol_node(path: &str, symbol_ref: SymbolRef, badges: Badges) -> TreeNode {
        TreeNode {
            kind: NodeKind::Symbol(symbol_ref),
            path: path.to_string(),
            badges,
            children: vec![],
            skip_reason: None,
            test_symbol_count: None,
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
    fn should_include_badge_labels_for_nonzero_badges_on_a_dir_row() {
        // ADR 0013 amendment (2026-07-13): the changed-symbol and fan-in
        // badges use `chg:` / `ref:` text labels instead of the original
        // `~` / `^` glyphs. `!{N}` (contract-change count) is
        // intentionally left as a compact glyph — see `push_badge_spans`'
        // doc comment for the scope split.
        let node = dir_node(
            "src",
            Badges {
                changed_symbols: 2,
                contract_changes: 1,
                fan_in: 3,
                ..Badges::default()
            },
            vec![file_node("src/a.rs", Badges::default())],
        );
        let row = Row {
            node: &node,
            depth: 0,
            expanded: true,
        };

        let line = entry_row_line(&row, "src", &HashMap::new(), false);

        assert_eq!("v src chg:2 !1 ref:3", line_text(&line));
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
    fn should_append_skip_reason_for_a_skipped_file_row() {
        let node = skipped_file_node("assets/logo.png", rinkaku_core::render::SkipReason::Binary);
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "assets/logo.png", &HashMap::new(), false);

        assert_eq!("  assets/logo.png  (skipped: binary)", line_text(&line));
    }

    #[test]
    fn should_dim_label_for_a_skipped_file_row() {
        let node = skipped_file_node("assets/logo.png", rinkaku_core::render::SkipReason::Binary);
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "assets/logo.png", &HashMap::new(), false);

        // The label span is the third span: indent, expand marker, label.
        assert_eq!(Some(Color::DarkGray), line.spans[2].style.fg);
    }

    #[test]
    fn should_not_append_skip_reason_for_an_ordinary_file_row() {
        let node = file_node("lib.rs", Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

        assert!(!line_text(&line).contains("skipped"));
    }

    #[test]
    fn should_append_test_badge_with_plural_symbols_noun_for_a_whole_test_file_row() {
        let node = test_file_node("src/lib_test.go", 3);
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "src/lib_test.go", &HashMap::new(), false);

        assert_eq!("  src/lib_test.go  [test] (3 symbols)", line_text(&line));
    }

    #[test]
    fn should_append_test_badge_with_singular_symbol_noun_when_count_is_one() {
        let node = test_file_node("src/lib_test.go", 1);
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "src/lib_test.go", &HashMap::new(), false);

        assert_eq!("  src/lib_test.go  [test] (1 symbol)", line_text(&line));
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
    fn should_not_strip_partial_string_overlap_between_sibling_directory_names() {
        // "src" and "src2" are two independent top-level roots (both
        // depth 0, i.e. siblings, not ancestor/descendant) that happen to
        // share "src" as a string prefix. `relative_labels` only ever
        // compares a row against its own ancestor chain (via
        // `ancestor_path_at[row.depth - 1]`), never against a sibling at
        // the same depth, so "src2" must keep its full label rather than
        // having "src" spuriously stripped off it as if "src" were its
        // parent.
        let src = dir_node("src", Badges::default(), vec![]);
        let src2 = dir_node("src2", Badges::default(), vec![]);
        let rows = vec![
            Row {
                node: &src,
                depth: 0,
                expanded: false,
            },
            Row {
                node: &src2,
                depth: 0,
                expanded: false,
            },
        ];

        let labels = relative_labels(&rows);

        assert_eq!(vec!["src".to_string(), "src2".to_string()], labels);
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

    // File-size warning badges (ADR 0028): a file row carrying a warning
    // renders `lines:{N}` with the numeric N colored by severity (yellow
    // for Warn, red for Split), and a directory row aggregates the
    // per-severity counts as `warn:N split:N` with the numbers colored
    // the same way. No emoji glyphs — the color already conveys severity
    // and emoji rendering width is inconsistent across terminals.

    /// Locates the styled span whose visible text is `content` in `line`,
    /// returning its foreground color (or `None` when the span exists but
    /// has no explicit fg, matching ratatui's default). Panics when no
    /// such span exists — a matching-span assertion is what the test
    /// wanted, so a missing span is a test failure, not a `None`.
    fn fg_of_span_with_content(line: &Line<'_>, content: &str) -> Option<Color> {
        line.spans
            .iter()
            .find(|span| span.content.as_ref() == content)
            .unwrap_or_else(|| {
                panic!(
                    "no span with content {content:?} found in line: {:?}",
                    line_text(line)
                )
            })
            .style
            .fg
    }

    #[test]
    fn should_render_lines_count_in_yellow_when_file_has_warn_severity() {
        let node = TreeNode {
            kind: NodeKind::File,
            path: "src/big.rs".to_string(),
            badges: Badges {
                own_file_size_severity: Some(FileSizeSeverity::Warn),
                own_file_line_count: Some(1734),
                ..Badges::default()
            },
            children: vec![],
            skip_reason: None,
            test_symbol_count: None,
        };
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "big.rs", &HashMap::new(), false);

        assert_eq!("  big.rs lines:1734", line_text(&line));
        // The numeric 1734 span carries the severity color; the leading
        // "lines:" label stays uncolored so the eye lands on the number.
        assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "1734"));
        assert_eq!(None, fg_of_span_with_content(&line, "lines:"));
    }

    #[test]
    fn should_render_lines_count_in_red_when_file_has_split_severity() {
        let node = TreeNode {
            kind: NodeKind::File,
            path: "src/huge.rs".to_string(),
            badges: Badges {
                own_file_size_severity: Some(FileSizeSeverity::Split),
                own_file_line_count: Some(4837),
                ..Badges::default()
            },
            children: vec![],
            skip_reason: None,
            test_symbol_count: None,
        };
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "huge.rs", &HashMap::new(), false);

        assert_eq!("  huge.rs lines:4837", line_text(&line));
        assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "4837"));
    }

    #[test]
    fn should_render_warn_and_split_labels_side_by_side_on_dir_row() {
        let node = dir_node(
            "src",
            Badges {
                file_size_warn_count: 2,
                file_size_split_count: 1,
                ..Badges::default()
            },
            vec![file_node("src/a.rs", Badges::default())],
        );
        let row = Row {
            node: &node,
            depth: 0,
            expanded: true,
        };

        let line = entry_row_line(&row, "src", &HashMap::new(), false);

        assert_eq!("v src warn:2 split:1", line_text(&line));
        // The numeric part of each half picks up its own severity color;
        // the "warn:" / "split:" labels themselves stay uncolored so the
        // eye lands on the counts.
        assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "2"));
        assert_eq!(Some(Color::Red), fg_of_span_with_content(&line, "1"));
        assert_eq!(None, fg_of_span_with_content(&line, "warn:"));
        assert_eq!(None, fg_of_span_with_content(&line, "split:"));
    }

    #[test]
    fn should_not_render_file_size_badge_when_file_node_has_no_warning() {
        let node = file_node("lib.rs", Badges::default());
        let row = Row {
            node: &node,
            depth: 0,
            expanded: false,
        };

        let line = entry_row_line(&row, "lib.rs", &HashMap::new(), false);

        let text = line_text(&line);
        assert!(!text.contains("lines:"));
        assert!(!text.contains("warn:"));
        assert!(!text.contains("split:"));
    }

    #[test]
    fn should_render_only_warn_label_on_dir_row_when_no_split_files() {
        // When only one severity is present under a directory, only that
        // half of the badge shows — the other half is omitted rather
        // than rendered as "warn:0" or "split:0".
        let node = dir_node(
            "src",
            Badges {
                file_size_warn_count: 3,
                file_size_split_count: 0,
                ..Badges::default()
            },
            vec![file_node("src/a.rs", Badges::default())],
        );
        let row = Row {
            node: &node,
            depth: 0,
            expanded: true,
        };

        let line = entry_row_line(&row, "src", &HashMap::new(), false);

        assert_eq!("v src warn:3", line_text(&line));
    }

    // ADR 0013 amendment (2026-07-13): `chg:N` and `ref:N` badges split
    // their label from their number across two spans so only the number
    // picks up cyan — matching the file-size badges' split-span pattern
    // (`lines:N`, `warn:N`, `split:N`). The label prefix reads at the
    // default color to keep the eye on the numeric part.
    #[test]
    fn should_color_only_the_number_of_chg_badge_and_leave_label_uncolored() {
        let node = dir_node(
            "src",
            Badges {
                changed_symbols: 299,
                ..Badges::default()
            },
            vec![file_node("src/a.rs", Badges::default())],
        );
        let row = Row {
            node: &node,
            depth: 0,
            expanded: true,
        };

        let line = entry_row_line(&row, "src", &HashMap::new(), false);

        assert_eq!("v src chg:299", line_text(&line));
        assert_eq!(Some(Color::Cyan), fg_of_span_with_content(&line, "299"));
        assert_eq!(None, fg_of_span_with_content(&line, "chg:"));
    }

    #[test]
    fn should_color_only_the_number_of_ref_badge_and_leave_label_uncolored() {
        let node = dir_node(
            "src",
            Badges {
                fan_in: 1072,
                ..Badges::default()
            },
            vec![file_node("src/a.rs", Badges::default())],
        );
        let row = Row {
            node: &node,
            depth: 0,
            expanded: true,
        };

        let line = entry_row_line(&row, "src", &HashMap::new(), false);

        assert_eq!("v src ref:1072", line_text(&line));
        assert_eq!(Some(Color::Cyan), fg_of_span_with_content(&line, "1072"));
        assert_eq!(None, fg_of_span_with_content(&line, "ref:"));
    }
}
