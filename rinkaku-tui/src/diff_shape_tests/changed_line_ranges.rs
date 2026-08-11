use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_return_empty_ranges_when_hunks_is_empty() {
    let actual = changed_line_ranges(&[]);

    assert_eq!(Vec::<(usize, usize)>::new(), actual);
}

#[test]
fn should_collect_single_range_from_a_single_hunk() {
    let h = hunk("@@ -1,1 +1,2 @@", Some((23, 41)), vec![""]);

    let actual = changed_line_ranges(&[&h]);

    assert_eq!(vec![(23, 41)], actual);
}

#[test]
fn should_sort_and_dedup_ranges() {
    let h1 = hunk("@@ -1,10 +1,10 @@", Some((3, 12)), vec![""]);
    let h2 = hunk("@@ -30,4 +27,4 @@", Some((27, 30)), vec![""]);

    let actual = changed_line_ranges(&[&h2, &h1]);

    assert_eq!(vec![(3, 12), (27, 30)], actual);
}

#[test]
fn should_exclude_zero_width_deletion_range() {
    // A pure-deletion hunk's `new_range` is `(start, start - 1)`
    // (`Hunk::new_range`'s own doc comment) — no visible new-side span
    // to name a *range* for, so it must not appear in the header.
    let h = hunk("@@ -10,3 +10,0 @@", Some((10, 9)), vec![""]);

    let actual = changed_line_ranges(&[&h]);

    assert_eq!(Vec::<(usize, usize)>::new(), actual);
}

#[test]
fn should_exclude_range_when_new_range_is_none() {
    let h = hunk("malformed", None, vec![""]);

    let actual = changed_line_ranges(&[&h]);

    assert_eq!(Vec::<(usize, usize)>::new(), actual);
}
