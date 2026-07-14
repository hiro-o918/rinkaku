//! The `?` help overlay's content (ADR 0020, glossary wording per ADR
//! 0023): a static keymap plus a short glossary, assembled as plain data so
//! `crate::ui` only has to lay it out, not decide what belongs in it.
//!
//! The keymap itself is fixed (not derived from `crate::app::InputKey` or
//! `crate::lib::translate_key` — both already carry their own doc comments
//! as the authoritative per-key rationale; this module is the *reviewer-
//! facing* summary of the same bindings, kept in sync by hand). Splitting
//! it into "Tree focus" / "Right focus" / "Global" groups mirrors ADR
//! 0020's own focus split, so the overlay reads as a direct answer to
//! "what does j/k do right now" rather than one flat undifferentiated list.

/// One row of the keymap: the key(s) as displayed text, and what they do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub keys: &'static str,
    pub description: &'static str,
}

/// One glossary entry: a term used elsewhere in the UI (an order mode name,
/// "blast radius", "cycle") paired with a short explanation — the answer to
/// "what does that word on the status line/tree actually mean".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryEntry {
    pub term: &'static str,
    pub explanation: &'static str,
}

/// One marker-legend entry: a tree row marker/badge's display text paired
/// with a short explanation of what it means — the visual-encoding
/// counterpart to [`GlossaryEntry`]'s concept glossary. `crate::ui::overlay`
/// renders `swatch` with the row's *real* style, looked up from
/// `crate::row_view`'s own style producers rather than duplicated here, so
/// this struct only needs to carry the text half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerLegendEntry {
    pub swatch: &'static str,
    pub explanation: &'static str,
}

/// One named group of [`KeyBinding`]s — "Tree focus", "Right focus", or
/// "Global" (ADR 0020's own focus/global split).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindingGroup {
    pub title: &'static str,
    pub bindings: &'static [KeyBinding],
}

/// The whole help overlay's content: every keymap group in display order,
/// then the markers legend, then the glossary. A `const`, not a function —
/// the content is fixed at compile time, so there is nothing to compute per
/// call.
pub struct HelpContent {
    pub keymap_groups: &'static [KeyBindingGroup],
    pub markers: &'static [MarkerLegendEntry],
    pub glossary: &'static [GlossaryEntry],
}

const TREE_FOCUS_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "j / k / ↓ / ↑",
        description: "Move the cursor",
    },
    KeyBinding {
        keys: "enter",
        description: "Expand/collapse a directory row, or open a file/symbol row (moves focus right)",
    },
    KeyBinding {
        keys: "space",
        description: "Expand/collapse a directory/file row (never moves focus)",
    },
    KeyBinding {
        keys: "e / E",
        description: "Expand every row",
    },
    KeyBinding {
        keys: "c / C",
        description: "Collapse every row",
    },
];

const RIGHT_FOCUS_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "j / k / ↓ / ↑",
        description: "Scroll the right pane by one line",
    },
    KeyBinding {
        keys: "ctrl-d / ctrl-u",
        description: "Scroll the right pane by half a page",
    },
    KeyBinding {
        keys: "gg / G",
        description: "Jump to the top / bottom of the right pane",
    },
    KeyBinding {
        keys: "h / esc",
        description: "Return focus to the tree",
    },
    KeyBinding {
        keys: "]",
        description: "Jump to the next hunk (Diff pane only)",
    },
    KeyBinding {
        keys: "[",
        description: "Jump to the previous hunk (Diff pane only)",
    },
];

const SOURCE_SCREEN_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "j / k / ↓ / ↑",
        description: "Scroll the source pane by one line",
    },
    KeyBinding {
        keys: "ctrl-d / ctrl-u",
        description: "Scroll the source pane by half a page",
    },
    KeyBinding {
        keys: "gg / G",
        description: "Jump to the top / bottom of the file",
    },
    KeyBinding {
        keys: "esc / q",
        description: "Return to the entry view",
    },
];

const REVIEW_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "n",
        description: "Compose a review note over the symbol under the cursor",
    },
    KeyBinding {
        keys: "N",
        description: "Open the review notes list",
    },
    KeyBinding {
        keys: "j/k, Enter, Esc, d",
        description: "Notes list: move, export, close, delete",
    },
];

const GLOBAL_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "d / D",
        description: "Toggle the right pane between Detail and Diff",
    },
    KeyBinding {
        keys: "r / R",
        description: "Toggle the right pane to the blast radius of the selected row",
    },
    KeyBinding {
        keys: "v / V",
        description: "Toggle unified/split (side-by-side) rendering — Diff pane and source view diff overlay alike",
    },
    KeyBinding {
        keys: "o / O",
        description: "Toggle topological/alphabetical ordering",
    },
    KeyBinding {
        keys: "s / S",
        description: "Open the source view for the symbol under the cursor",
    },
    KeyBinding {
        keys: "gd",
        description: "Jump to a callee of the symbol under the cursor",
    },
    KeyBinding {
        keys: "gr",
        description: "Jump to a caller of the symbol under the cursor",
    },
    KeyBinding {
        keys: "ctrl-o",
        description: "Jump back to the previous location in the jumplist",
    },
    KeyBinding {
        keys: "ctrl-i",
        description: "Jump forward to the next location in the jumplist",
    },
    KeyBinding {
        keys: "?",
        description: "Toggle this help overlay",
    },
    KeyBinding {
        keys: "q / ctrl-c",
        description: "Quit (esc/q return to the entry view from the source view)",
    },
];

const KEYMAP_GROUPS: &[KeyBindingGroup] = &[
    KeyBindingGroup {
        title: "Tree focus",
        bindings: TREE_FOCUS_BINDINGS,
    },
    KeyBindingGroup {
        title: "Right focus",
        bindings: RIGHT_FOCUS_BINDINGS,
    },
    KeyBindingGroup {
        title: "Source view",
        bindings: SOURCE_SCREEN_BINDINGS,
    },
    KeyBindingGroup {
        title: "Review",
        bindings: REVIEW_BINDINGS,
    },
    KeyBindingGroup {
        title: "Global",
        bindings: GLOBAL_BINDINGS,
    },
];

/// The tree pane's marker/badge legend, in the same added-like →
/// changed-like → removed-like → aggregates reading order as the mermaid
/// legend (ADR 0039/0040) — the visual-encoding reference for every marker
/// `crate::row_view::entry_row_line` can draw. `swatch` is the display text
/// only; `crate::ui::overlay` pairs each with its real style from
/// `crate::row_view`.
const MARKER_LEGEND: &[MarkerLegendEntry] = &[
    MarkerLegendEntry {
        swatch: "v / >",
        explanation: "Expand marker: children shown / hidden (blank = leaf, nothing to expand)",
    },
    MarkerLegendEntry {
        swatch: "fn struct enum trait class iface type",
        explanation: "Symbol row's kind prefix, abbreviated from the language's own keyword",
    },
    MarkerLegendEntry {
        swatch: "+",
        explanation: "Added symbol",
    },
    MarkerLegendEntry {
        swatch: "~",
        explanation: "Signature-changed symbol",
    },
    MarkerLegendEntry {
        swatch: "(dimmed name)",
        explanation: "Body-only, unclassified, or test symbol — exists, but carries less review weight",
    },
    MarkerLegendEntry {
        swatch: "x",
        explanation: "Removed symbol",
    },
    MarkerLegendEntry {
        swatch: "(dimmed + struck-through name)",
        explanation: "Removed symbol's name",
    },
    MarkerLegendEntry {
        swatch: "(cycle)",
        explanation: "Directory contains a dependency cycle",
    },
    MarkerLegendEntry {
        swatch: "!",
        explanation: "Risk marker: a contract change and a high-fan-in symbol in the same subtree",
    },
    MarkerLegendEntry {
        swatch: "[test] (N symbols)",
        explanation: "Whole-test-file badge",
    },
    MarkerLegendEntry {
        swatch: "N tests",
        explanation: "Collapsed group of a file's test symbols",
    },
    MarkerLegendEntry {
        swatch: "(skipped: ...)",
        explanation: "Reason a file was not analyzed (parse failure, unsupported language, ...)",
    },
    MarkerLegendEntry {
        swatch: "chg:N",
        explanation: "Changed, non-removed symbols in this subtree",
    },
    MarkerLegendEntry {
        swatch: "api:N",
        explanation: "Contract changes in this subtree: signature-changed symbols plus removed symbols",
    },
    MarkerLegendEntry {
        swatch: "fan-in:N",
        explanation: "Sum of used_by counts over every high-fan-in symbol in this subtree",
    },
    MarkerLegendEntry {
        swatch: "lines:N",
        explanation: "This file's own line count, colored by file-size band (normal/watch/warn/split)",
    },
    MarkerLegendEntry {
        swatch: "warn:N",
        explanation: "Directory rows: count of Warn-band files in this subtree",
    },
    MarkerLegendEntry {
        swatch: "split:N",
        explanation: "Directory rows: count of Split-band files in this subtree",
    },
];

const GLOSSARY: &[GlossaryEntry] = &[
    GlossaryEntry {
        term: "topological order",
        explanation: "Directories ordered least-depended-on first, foundations last",
    },
    GlossaryEntry {
        term: "alphabetical order",
        explanation: "Directories ordered A-Z, ignoring dependency direction",
    },
    GlossaryEntry {
        term: "blast radius",
        explanation: "The dependency tree rooted at a selected directory or file, showing what would be affected if it changed",
    },
    GlossaryEntry {
        term: "cycle",
        explanation: "A dependency loop: two or more symbols depend on each other, so the tree stops and points back to where it first appeared",
    },
    GlossaryEntry {
        term: "jumplist",
        explanation: "The history of gd/gr jump locations — ctrl-o/ctrl-i move back/forward through it",
    },
];

/// The whole help overlay's content (module doc comment).
pub const HELP_CONTENT: HelpContent = HelpContent {
    keymap_groups: KEYMAP_GROUPS,
    markers: MARKER_LEGEND,
    glossary: GLOSSARY,
};

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_list_keymap_groups_in_tree_right_source_review_global_order() {
        let titles: Vec<&str> = HELP_CONTENT
            .keymap_groups
            .iter()
            .map(|group| group.title)
            .collect();

        assert_eq!(
            vec![
                "Tree focus",
                "Right focus",
                "Source view",
                "Review",
                "Global"
            ],
            titles
        );
    }

    #[test]
    fn should_document_source_view_scroll_bindings_in_the_source_view_group() {
        // ADR 0026: the source view has its own scroll bindings (j/k,
        // Ctrl-d/Ctrl-u, gg/G) plus esc/q to return to the entry view.
        // Pinned so a future rename/typo/omission of any of them is
        // caught, and so the group's own presence is not silently
        // dropped by a keymap refactor.
        let source_view = HELP_CONTENT
            .keymap_groups
            .iter()
            .find(|group| group.title == "Source view")
            .expect("Source view group present");

        let keys: Vec<&str> = source_view
            .bindings
            .iter()
            .map(|binding| binding.keys)
            .collect();

        assert!(keys.contains(&"j / k / ↓ / ↑"));
        assert!(keys.contains(&"ctrl-d / ctrl-u"));
        assert!(keys.contains(&"gg / G"));
        assert!(keys.contains(&"esc / q"));
    }

    #[test]
    fn should_have_no_empty_keymap_group() {
        for group in HELP_CONTENT.keymap_groups {
            assert!(
                !group.bindings.is_empty(),
                "group {:?} has no bindings",
                group.title
            );
        }
    }

    #[test]
    fn should_order_marker_legend_added_changed_removed_then_aggregates() {
        let swatches: Vec<&str> = HELP_CONTENT
            .markers
            .iter()
            .map(|entry| entry.swatch)
            .collect();

        assert_eq!(
            vec![
                "v / >",
                "fn struct enum trait class iface type",
                "+",
                "~",
                "(dimmed name)",
                "x",
                "(dimmed + struck-through name)",
                "(cycle)",
                "!",
                "[test] (N symbols)",
                "N tests",
                "(skipped: ...)",
                "chg:N",
                "api:N",
                "fan-in:N",
                "lines:N",
                "warn:N",
                "split:N",
            ],
            swatches
        );
    }

    #[test]
    fn should_describe_api_badge_as_signature_changed_plus_removed_symbols() {
        let entry = HELP_CONTENT
            .markers
            .iter()
            .find(|entry| entry.swatch == "api:N")
            .expect("api:N entry present");

        assert!(entry.explanation.contains("removed"));
        assert!(entry.explanation.contains("signature-changed"));
    }

    #[test]
    fn should_describe_fan_in_badge_as_a_sum_over_high_fan_in_symbols() {
        let entry = HELP_CONTENT
            .markers
            .iter()
            .find(|entry| entry.swatch == "fan-in:N")
            .expect("fan-in:N entry present");

        assert!(entry.explanation.contains("Sum"));
        assert!(entry.explanation.contains("high-fan-in"));
    }

    #[test]
    fn should_include_a_glossary_entry_for_blast_radius_and_cycle() {
        let terms: Vec<&str> = HELP_CONTENT
            .glossary
            .iter()
            .map(|entry| entry.term)
            .collect();

        assert!(terms.contains(&"blast radius"));
        assert!(terms.contains(&"cycle"));
    }

    #[test]
    fn should_include_a_glossary_entry_for_jumplist() {
        let terms: Vec<&str> = HELP_CONTENT
            .glossary
            .iter()
            .map(|entry| entry.term)
            .collect();

        assert!(terms.contains(&"jumplist"));
    }

    #[test]
    fn should_document_gd_gr_and_jumplist_bindings_in_the_global_group() {
        let global = HELP_CONTENT
            .keymap_groups
            .iter()
            .find(|group| group.title == "Global")
            .expect("Global group present");

        let keys: Vec<&str> = global.bindings.iter().map(|binding| binding.keys).collect();

        assert!(keys.contains(&"gd"));
        assert!(keys.contains(&"gr"));
        assert!(keys.contains(&"ctrl-o"));
        assert!(keys.contains(&"ctrl-i"));
    }

    #[test]
    fn should_document_review_notes_bindings_in_a_review_group() {
        let review = HELP_CONTENT
            .keymap_groups
            .iter()
            .find(|group| group.title == "Review")
            .expect("Review group present");

        let keys: Vec<&str> = review.bindings.iter().map(|binding| binding.keys).collect();

        assert!(keys.contains(&"n"));
        assert!(keys.contains(&"N"));
        assert!(keys.contains(&"j/k, Enter, Esc, d"));
    }

    #[test]
    fn should_document_h_and_esc_as_the_return_to_tree_binding_in_right_focus_group() {
        let right_focus = HELP_CONTENT
            .keymap_groups
            .iter()
            .find(|group| group.title == "Right focus")
            .expect("Right focus group present");

        let has_return_binding = right_focus
            .bindings
            .iter()
            .any(|binding| binding.keys == "h / esc");

        assert!(has_return_binding);
    }
}
