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
/// - All three badges use text labels (`chg:` / `api:` / `fan-in:`)
///   matching the file-size badges' `lines:` / `warn:` / `split:`
///   convention (see ADR 0013 amendments 2026-07-13 and
///   feat/label-contract-changes-badge). The single-glyph prefixes they
///   originally replaced (`~` for changed, `!` for contract change, `^`
///   for fan-in) conveyed no semantic hint on their own to a first-time
///   reviewer — `!` in particular read as generic "warning" rather than
///   pointing at *what* changed. The fan-in badge's label itself was
///   later relabeled again, from `ref:` to `fan-in:` (ADR 0033): `ref:`
///   collided visually with the unrelated `gr` ("go to references")
///   keybinding despite naming a different concept, and "hotspot" (the
///   underlying aggregation's original name) collided with an unrelated
///   well-known term (CodeScene's change-frequency metric) — "fan-in"
///   has neither problem and matches the detail pane's own `fan-in: N`
///   wording. The label stays default color so the eye lands on the
///   number, matching the file-size badges' split-span pattern.
/// - `chg:`/`fan-in:` numbers are cyan (informational counts), but
///   `api:` is yellow — the same warning color as the file-size `warn:`
///   badge below — because a contract change (signature-changed or
///   removed symbol) is the one badge that flags something a caller
///   should double-check, restoring in color the "pay attention" signal
///   the original `!` glyph carried on its own.
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
        spans.push(Span::raw("api:"));
        spans.push(Span::styled(
            badges.contract_changes.to_string(),
            Style::default().fg(Color::Yellow),
        ));
        wrote_any_ascii_badge = true;
    }
    if badges.fan_in > 0 {
        if wrote_any_ascii_badge {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::raw("fan-in:"));
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
#[path = "row_view_tests/mod.rs"]
mod tests;
