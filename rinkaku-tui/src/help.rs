//! The `?` help overlay's content (ADR 0020): a static keymap plus a short
//! glossary, assembled as plain data so `crate::ui` only has to lay it out,
//! not decide what belongs in it.
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
/// "pivot", "cycle") paired with a short explanation — the answer to
/// "what does that word on the status line/tree actually mean".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryEntry {
    pub term: &'static str,
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
/// then the glossary. A `const`, not a function — the content is fixed at
/// compile time, so there is nothing to compute per call.
pub struct HelpContent {
    pub keymap_groups: &'static [KeyBindingGroup],
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

const GLOBAL_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "d / D",
        description: "Toggle the right pane between Detail and Diff",
    },
    KeyBinding {
        keys: "p / P",
        description: "Toggle the right pane to the pivot tree rooted at the selected row",
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
        title: "Global",
        bindings: GLOBAL_BINDINGS,
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
        term: "pivot",
        explanation: "Re-roots the dependency tree at the selected directory/file's path",
    },
    GlossaryEntry {
        term: "cycle",
        explanation: "A closing back-edge in the dependency graph — two or more directories depend on each other",
    },
    GlossaryEntry {
        term: "jumplist",
        explanation: "The history of gd/gr jump locations — ctrl-o/ctrl-i move back/forward through it",
    },
];

/// The whole help overlay's content (module doc comment).
pub const HELP_CONTENT: HelpContent = HelpContent {
    keymap_groups: KEYMAP_GROUPS,
    glossary: GLOSSARY,
};

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_include_a_tree_focus_right_focus_and_global_group_in_that_order() {
        let titles: Vec<&str> = HELP_CONTENT
            .keymap_groups
            .iter()
            .map(|group| group.title)
            .collect();

        assert_eq!(vec!["Tree focus", "Right focus", "Global"], titles);
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
    fn should_include_a_glossary_entry_for_pivot_and_cycle() {
        let terms: Vec<&str> = HELP_CONTENT
            .glossary
            .iter()
            .map(|entry| entry.term)
            .collect();

        assert!(terms.contains(&"pivot"));
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
