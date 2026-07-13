// ADR 0013 amendments (2026-07-13, feat/label-contract-changes-badge):
// `chg:N`, `api:N`, and `ref:N` badges split their label from their
// number across two spans so only the number is colored — matching the
// file-size badges' split-span pattern (`lines:N`, `warn:N`, `split:N`).
// The label prefix reads at the default color to keep the eye on the
// numeric part. `chg:`/`ref:` are cyan (informational counts); `api:`
// is yellow, matching the file-size `warn:` badge's warning color,
// since a contract change is the one badge meant to catch attention.

use super::*;

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

#[test]
fn should_color_only_the_number_of_api_badge_yellow_and_leave_label_uncolored() {
    // Yellow (not cyan, unlike chg:/ref:) — see this file's header
    // comment: api: is the one badge meant to flag something worth a
    // second look, so it borrows the file-size warn: badge's color.
    let node = dir_node(
        "src",
        Badges {
            contract_changes: 42,
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

    assert_eq!("v src api:42", line_text(&line));
    assert_eq!(Some(Color::Yellow), fg_of_span_with_content(&line, "42"));
    assert_eq!(None, fg_of_span_with_content(&line, "api:"));
}
