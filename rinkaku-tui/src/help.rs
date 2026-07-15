//! The `?` help overlay's content (ADR 0020, glossary wording per ADR
//! 0023): a keymap plus a short glossary, assembled as plain data so
//! `crate::ui` only has to lay it out, not decide what belongs in it.
//!
//! The keymap itself is fixed (not derived from `crate::app::InputKey` or
//! `crate::lib::translate_key` — both already carry their own doc comments
//! as the authoritative per-key rationale; this module is the *reviewer-
//! facing* summary of the same bindings, kept in sync by hand). Splitting
//! it into "Tree focus" / "Right focus" / "Global" groups mirrors ADR
//! 0020's own focus split, so the overlay reads as a direct answer to
//! "what does j/k do right now" rather than one flat undifferentiated list.
//!
//! [`help_content`] is a function, not a `const`, because its description/
//! explanation strings are looked up per [`crate::locale::Locale`] via
//! `rust_i18n::t!` (ADR 0055), which allocates and so cannot run in const
//! context. Key labels (`keys`, `swatch`, `term`) stay `&'static str` —
//! ADR 0055 scopes translation to prose, not key labels or term names.

use crate::locale::Locale;

/// One row of the keymap: the key(s) as displayed text, and what they do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub keys: &'static str,
    pub description: String,
}

/// One glossary entry: a term used elsewhere in the UI (an order mode name,
/// "blast radius", "cycle") paired with a short explanation — the answer to
/// "what does that word on the status line/tree actually mean".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlossaryEntry {
    pub term: &'static str,
    pub explanation: String,
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
    pub explanation: String,
}

/// One named group of [`KeyBinding`]s — "Tree focus", "Right focus", or
/// "Global" (ADR 0020's own focus/global split).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindingGroup {
    pub title: String,
    pub bindings: Vec<KeyBinding>,
}

/// The whole help overlay's content: every keymap group in display order,
/// then the markers legend, then the glossary — see [`help_content`].
pub struct HelpContent {
    pub keymap_groups: Vec<KeyBindingGroup>,
    pub markers: Vec<MarkerLegendEntry>,
    pub glossary: Vec<GlossaryEntry>,
}

fn tree_focus_bindings(locale: Locale) -> Vec<KeyBinding> {
    let tag = locale.tag();
    vec![
        KeyBinding {
            keys: "j / k / ↓ / ↑",
            description: rust_i18n::t!("help.binding.move_cursor", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "enter",
            description: rust_i18n::t!("help.binding.expand_collapse_open", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "space",
            description: rust_i18n::t!("help.binding.expand_collapse_row", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "e / E",
            description: rust_i18n::t!("help.binding.expand_every_row", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "c / C",
            description: rust_i18n::t!("help.binding.collapse_every_row", locale = tag)
                .into_owned(),
        },
    ]
}

fn right_focus_bindings(locale: Locale) -> Vec<KeyBinding> {
    let tag = locale.tag();
    vec![
        KeyBinding {
            keys: "j / k / ↓ / ↑",
            description: rust_i18n::t!("help.binding.scroll_right_pane_line", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "ctrl-d / ctrl-u",
            description: rust_i18n::t!("help.binding.scroll_right_pane_half_page", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "gg / G",
            description: rust_i18n::t!("help.binding.jump_right_pane_top_bottom", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "h / esc",
            description: rust_i18n::t!("help.binding.return_focus_to_tree", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "]",
            description: rust_i18n::t!("help.binding.jump_next_hunk", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "[",
            description: rust_i18n::t!("help.binding.jump_previous_hunk", locale = tag)
                .into_owned(),
        },
    ]
}

fn source_screen_bindings(locale: Locale) -> Vec<KeyBinding> {
    let tag = locale.tag();
    vec![
        KeyBinding {
            keys: "j / k / ↓ / ↑",
            description: rust_i18n::t!("help.binding.scroll_source_pane_line", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "ctrl-d / ctrl-u",
            description: rust_i18n::t!("help.binding.scroll_source_pane_half_page", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "gg / G",
            description: rust_i18n::t!("help.binding.jump_file_top_bottom", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "esc / q",
            description: rust_i18n::t!("help.binding.return_to_entry_view", locale = tag)
                .into_owned(),
        },
    ]
}

fn review_bindings(locale: Locale) -> Vec<KeyBinding> {
    let tag = locale.tag();
    vec![
        KeyBinding {
            keys: "n",
            description: rust_i18n::t!("help.binding.compose_review_note", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "N",
            description: rust_i18n::t!("help.binding.open_review_notes_list", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "j/k, Enter, Esc, d",
            description: rust_i18n::t!("help.binding.notes_list_actions", locale = tag)
                .into_owned(),
        },
    ]
}

fn global_bindings(locale: Locale) -> Vec<KeyBinding> {
    let tag = locale.tag();
    vec![
        KeyBinding {
            keys: "d / D",
            description: rust_i18n::t!("help.binding.toggle_detail_diff", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "r / R",
            description: rust_i18n::t!("help.binding.toggle_blast_radius", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "v / V",
            description: rust_i18n::t!("help.binding.toggle_unified_split", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "o / O",
            description: rust_i18n::t!("help.binding.toggle_order_mode", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "s / S",
            description: rust_i18n::t!("help.binding.open_source_view", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "gd",
            description: rust_i18n::t!("help.binding.jump_to_callee", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "gr",
            description: rust_i18n::t!("help.binding.jump_to_caller", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "ctrl-o",
            description: rust_i18n::t!("help.binding.jump_back", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "ctrl-i",
            description: rust_i18n::t!("help.binding.jump_forward", locale = tag).into_owned(),
        },
        KeyBinding {
            keys: "w / W",
            description: rust_i18n::t!("help.binding.open_pr_in_browser", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "U",
            description: rust_i18n::t!("help.binding.prompt_self_update", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "?",
            description: rust_i18n::t!("help.binding.toggle_help_overlay", locale = tag)
                .into_owned(),
        },
        KeyBinding {
            keys: "q / ctrl-c",
            description: rust_i18n::t!("help.binding.quit", locale = tag).into_owned(),
        },
    ]
}

fn keymap_groups(locale: Locale) -> Vec<KeyBindingGroup> {
    let tag = locale.tag();
    vec![
        KeyBindingGroup {
            title: rust_i18n::t!("help.group.tree_focus", locale = tag).into_owned(),
            bindings: tree_focus_bindings(locale),
        },
        KeyBindingGroup {
            title: rust_i18n::t!("help.group.right_focus", locale = tag).into_owned(),
            bindings: right_focus_bindings(locale),
        },
        KeyBindingGroup {
            title: rust_i18n::t!("help.group.source_view", locale = tag).into_owned(),
            bindings: source_screen_bindings(locale),
        },
        KeyBindingGroup {
            title: rust_i18n::t!("help.group.review", locale = tag).into_owned(),
            bindings: review_bindings(locale),
        },
        KeyBindingGroup {
            title: rust_i18n::t!("help.group.global", locale = tag).into_owned(),
            bindings: global_bindings(locale),
        },
    ]
}

/// The tree pane's marker/badge legend, in the same added-like →
/// changed-like → removed-like → aggregates reading order as the mermaid
/// legend (ADR 0039/0040) — the visual-encoding reference for every marker
/// `crate::row_view::entry_row_line` can draw. `swatch` is the display text
/// only; `crate::ui::overlay` pairs each with its real style from
/// `crate::row_view`.
fn marker_legend(locale: Locale) -> Vec<MarkerLegendEntry> {
    let tag = locale.tag();
    vec![
        MarkerLegendEntry {
            swatch: "v / >",
            explanation: rust_i18n::t!("help.marker.expand_marker", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "fn struct enum trait class iface type",
            explanation: rust_i18n::t!("help.marker.symbol_kind_prefix", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "+",
            explanation: rust_i18n::t!("help.marker.added_symbol", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "~",
            explanation: rust_i18n::t!("help.marker.signature_changed_symbol", locale = tag)
                .into_owned(),
        },
        MarkerLegendEntry {
            swatch: "(dimmed name)",
            explanation: rust_i18n::t!("help.marker.body_only_symbol", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "x",
            explanation: rust_i18n::t!("help.marker.removed_symbol", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "(dimmed + struck-through name)",
            explanation: rust_i18n::t!("help.marker.removed_symbol_name", locale = tag)
                .into_owned(),
        },
        MarkerLegendEntry {
            swatch: "(cycle)",
            explanation: rust_i18n::t!("help.marker.dependency_cycle", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "!",
            explanation: rust_i18n::t!("help.marker.risk_marker", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "[test] (N symbols)",
            explanation: rust_i18n::t!("help.marker.whole_test_file_badge", locale = tag)
                .into_owned(),
        },
        MarkerLegendEntry {
            swatch: "N tests",
            explanation: rust_i18n::t!("help.marker.collapsed_test_group", locale = tag)
                .into_owned(),
        },
        MarkerLegendEntry {
            swatch: "(skipped: ...)",
            explanation: rust_i18n::t!("help.marker.skipped_reason", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "chg:N",
            explanation: rust_i18n::t!("help.marker.changed_count", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "api:N",
            explanation: rust_i18n::t!("help.marker.api_count", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "fan-in:N",
            explanation: rust_i18n::t!("help.marker.fan_in_count", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "lines:N",
            explanation: rust_i18n::t!("help.marker.lines_count", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "warn:N",
            explanation: rust_i18n::t!("help.marker.warn_count", locale = tag).into_owned(),
        },
        MarkerLegendEntry {
            swatch: "split:N",
            explanation: rust_i18n::t!("help.marker.split_count", locale = tag).into_owned(),
        },
    ]
}

fn glossary(locale: Locale) -> Vec<GlossaryEntry> {
    let tag = locale.tag();
    vec![
        GlossaryEntry {
            term: "topological order",
            explanation: rust_i18n::t!("help.glossary.topological_order", locale = tag)
                .into_owned(),
        },
        GlossaryEntry {
            term: "alphabetical order",
            explanation: rust_i18n::t!("help.glossary.alphabetical_order", locale = tag)
                .into_owned(),
        },
        GlossaryEntry {
            term: "blast radius",
            explanation: rust_i18n::t!("help.glossary.blast_radius", locale = tag).into_owned(),
        },
        GlossaryEntry {
            term: "cycle",
            explanation: rust_i18n::t!("help.glossary.cycle", locale = tag).into_owned(),
        },
        GlossaryEntry {
            term: "jumplist",
            explanation: rust_i18n::t!("help.glossary.jumplist", locale = tag).into_owned(),
        },
    ]
}

/// Builds the whole help overlay's content for `locale` (module doc comment
/// on why this is a function rather than the `const` it used to be).
pub fn help_content(locale: Locale) -> HelpContent {
    HelpContent {
        keymap_groups: keymap_groups(locale),
        markers: marker_legend(locale),
        glossary: glossary(locale),
    }
}

#[cfg(test)]
#[path = "help/tests.rs"]
mod tests;
