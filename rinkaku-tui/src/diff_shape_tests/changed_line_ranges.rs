use super::*;
use pretty_assertions::assert_eq;

fn section(symbol_id: Option<&str>, hunks: Vec<AttributedHunk>) -> DiffSection {
    DiffSection {
        title: symbol_id.unwrap_or(MODULE_LEVEL_TITLE).to_string(),
        symbol_id: symbol_id.map(str::to_string),
        contract_header: None,
        hunks,
    }
}

#[test]
fn should_return_empty_ranges_when_sections_is_empty() {
    let actual = changed_line_ranges(&[]);

    assert_eq!(Vec::<(usize, usize)>::new(), actual);
}

#[test]
fn should_collect_single_range_from_a_single_hunk() {
    let s = section(
        Some("lib.rs::foo"),
        vec![attributed(
            0,
            hunk("@@ -1,1 +1,2 @@", Some((23, 41)), vec![""]),
        )],
    );

    let actual = changed_line_ranges(&[&s]);

    assert_eq!(vec![(23, 41)], actual);
}

#[test]
fn should_sort_and_dedup_ranges_when_hunks_are_cloned_across_sections() {
    // ADR 0029: a hunk that overlaps multiple symbols is cloned into
    // each owning section — the same `new_range` therefore appears in
    // both `s1.hunks` and `s2.hunks`. `changed_line_ranges` must collapse
    // that back to one entry so the header's `range:` line doesn't repeat
    // it. Also pins the sort: `s2`'s range is written first in the input
    // to catch a bug that would preserve declared order.
    let shared_hunk = hunk("@@ -1,10 +1,10 @@", Some((3, 12)), vec![""]);
    let s1 = section(
        Some("lib.rs::foo"),
        vec![
            attributed(0, shared_hunk.clone()),
            attributed(1, hunk("@@ -30,4 +27,4 @@", Some((27, 30)), vec![""])),
        ],
    );
    let s2 = section(Some("lib.rs::bar"), vec![attributed(0, shared_hunk)]);

    let actual = changed_line_ranges(&[&s2, &s1]);

    assert_eq!(vec![(3, 12), (27, 30)], actual);
}

#[test]
fn should_exclude_zero_width_deletion_range() {
    // A pure-deletion hunk's `new_range` is `(start, start - 1)`
    // (`Hunk::new_range`'s own doc comment) — no visible new-side span
    // to name a *range* for, so it must not appear in the header.
    let s = section(
        None,
        vec![attributed(
            0,
            hunk("@@ -10,3 +10,0 @@", Some((10, 9)), vec![""]),
        )],
    );

    let actual = changed_line_ranges(&[&s]);

    assert_eq!(Vec::<(usize, usize)>::new(), actual);
}

#[test]
fn should_exclude_range_when_new_range_is_none() {
    let s = section(None, vec![attributed(0, hunk("malformed", None, vec![""]))]);

    let actual = changed_line_ranges(&[&s]);

    assert_eq!(Vec::<(usize, usize)>::new(), actual);
}
