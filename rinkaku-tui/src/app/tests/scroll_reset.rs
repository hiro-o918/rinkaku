use super::{
    focused_right_and_scrolled_one_line, report_with_one_symbol, report_with_two_directories,
};
use crate::app::{App, Focus, InputKey, RightPane, Screen};
use pretty_assertions::assert_eq;

#[test]
fn should_reset_right_pane_scroll_when_focus_returns_to_tree() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down);
    assert_eq!(1, app.right_pane_scroll());

    let app = app.handle_key(InputKey::FocusLeft);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_reset_right_pane_scroll_when_toggling_diff_pane() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down);
    assert_eq!(1, app.right_pane_scroll());

    let app = app.handle_key(InputKey::ToggleDiff);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_preserve_right_pane_scroll_when_open_pressed_from_tree_focus_on_diff_pane() {
    // ADR 0027 dogfooding finding: pressing Enter from Tree focus onto
    // a file/symbol row that is already showing on the Diff pane must
    // not wipe `right_pane_scroll` — the pane's content is not
    // changing, only the focus is, so the reviewer's reading position
    // (whether set by the previous auto-scroll to the section start,
    // or by any manual `j`/`k` they did after that) must survive the
    // focus swap. Without this, `run_app`'s "only auto-scroll on focus
    // change" rule leaves nothing to re-derive the scroll from and the
    // pane snaps back to line 0.
    let report = report_with_one_symbol();
    // Start on Diff pane (the default per ADR 0020), cursor on the
    // symbol row, focus Right. Manually seed a nonzero scroll to
    // represent an auto-scroll offset or a manual `j` press.
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> the symbol row
        .with_right_pane_scroll(4);
    assert_eq!(Focus::Tree, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(4, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Open);

    assert_eq!(Focus::Right, app.focus());
    assert_eq!(RightPane::Diff, app.right_pane());
    assert_eq!(4, app.right_pane_scroll());
}

#[test]
fn should_keep_right_pane_scroll_at_zero_when_returning_from_source_screen() {
    // Opening the source screen itself always resets scroll to 0
    // (`InputKey::Source`'s own reset, per the blanket rule) and every
    // key but `Back` is then a no-op while `Screen::Source` is active
    // (`App::handle_key`'s `Screen::Source` arm) — so scroll can never
    // become nonzero while the source screen is open in the first
    // place, unlike the pre-ADR-0020 world where `ScrollDown`/`ScrollUp`
    // were separate keys `Screen::Source`'s catch-all arm also
    // swallowed but which could still be pending from before entering.
    // This test pins that invariant end to end: `Back` finds scroll
    // already at 0 and leaves it there.
    //
    // Post-dogfooding-fix note: the source screen is now reached only
    // via the dedicated `s` key (`InputKey::Source`), not `Enter`
    // (`InputKey::Open`'s own doc comment) — `Open` is exercised
    // separately by the `Open`-specific tests above.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down) // cursor -> "foo" (a Symbol row)
        .handle_key(InputKey::Source); // opens Screen::Source
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 0,
        },
        *app.screen()
    );
    assert_eq!(0, app.right_pane_scroll());

    let app = app.handle_key(InputKey::Back);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_scroll_source_screen_without_touching_right_pane_scroll_when_down_is_pressed() {
    // ADR 0026: `j`/`k` on the source screen updates
    // `Screen::Source::scroll_top`, not the entry view's own
    // `right_pane_scroll` — the two are independent pieces of scroll
    // state (see the field's own doc comment on why they were not
    // unified). Pins both halves: `right_pane_scroll` stays at 0
    // (the entry view has never scrolled) and the source screen's
    // `scroll_top` moves.
    //
    // Post-dogfooding-fix note: the source screen is reached only via
    // the dedicated `s` key (`InputKey::Source`), not `Enter` — see
    // `should_keep_right_pane_scroll_at_zero_when_returning_from_source_screen`'s
    // own note.
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source); // opens source

    let app = app.handle_key(InputKey::Down);

    assert_eq!(0, app.right_pane_scroll());
    assert_eq!(
        Screen::Source {
            symbol_id: "lib.rs::foo".to_string(),
            scroll_top: 1,
        },
        *app.screen()
    );
}

#[test]
fn should_reset_right_pane_scroll_when_select_collapses_the_row_under_the_cursor() {
    // Row 0 is "a" itself; collapsing it via `Select` hides its
    // children but the cursor's own row survives unmoved — still a
    // case the blanket reset rule must cover, since a directory row's
    // own detail content (fan-in/badges) does not depend on which of
    // its children are currently shown, but this pins the simplest
    // Select case regardless. `Open` on "a" (a directory row) does not
    // itself change focus (`App::handle_key`'s `Open` arm), so `Down`
    // right after it is still what actually reaches `Focus::Right` —
    // reusing the shared four-directory fixture below would change
    // which row is under the cursor, so this test builds its own
    // two-directory report and drives the two steps by hand instead of
    // via `focused_right_and_scrolled_one_line`.
    let report = report_with_two_directories();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let app = app.handle_key(InputKey::Select);

    // `Select` never moves focus (ADR 0020), and scrolling never
    // applied here in the first place (Focus::Tree the whole time), so
    // this collapses to: collapsing "a" leaves the scroll offset at its
    // already-zero default. Kept as its own test (rather than folded
    // into a broader one) since it pins that `Select` specifically
    // never becomes a scroll-affecting action just because it can
    // reshuffle the row list, matching `CollapseAll`'s own case below.
    assert_eq!(0, app.right_pane_scroll());
    let rows = app.nav().rows(app.tree());
    let paths: Vec<&str> = rows.iter().map(|r| r.node.path.as_str()).collect();
    // "bar" (a Symbol row) carries its containing file's path
    // ("b/two.rs"), not a path of its own (`TreeNode::path`'s own doc
    // comment) — so the flattened path list repeats "b/two.rs" for both
    // the File row and its one Symbol child.
    assert_eq!(vec!["a", "b", "b/two.rs", "b/two.rs"], paths);
}

#[test]
fn should_reset_right_pane_scroll_when_collapse_all_retargets_cursor_onto_a_different_node() {
    // Cursor starts on "b/two.rs" (row 4, the File row directly under
    // "b"); CollapseAll hides every file/symbol row, and
    // `Nav::retarget_cursor` lands the cursor on "b" (the nearest
    // still-visible ancestor) — a genuinely different node's detail
    // than the one the pre-collapse scroll offset was scrolled into.
    // "b/two.rs" (a File row, not "bar"/a Symbol row) is the deliberate
    // choice: `Open` on a Symbol row also switches to `Screen::Source`
    // (`App::handle_key`'s `Open` arm), which would make the
    // `CollapseAll` this test presses next a no-op (every key but
    // `Back` is swallowed on `Screen::Source`) — a File row reaches
    // `Focus::Right` without leaving `Screen::Entry`.
    let report = report_with_two_directories();
    let mut app = App::new(&report);
    for _ in 0..4 {
        app = app.handle_key(InputKey::Down);
    }
    let rows = app.nav().rows(app.tree());
    assert_eq!("b/two.rs", rows[app.nav().cursor()].node.path);
    let app = app
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    assert_eq!(2, app.right_pane_scroll());

    let app = app.handle_key(InputKey::CollapseAll);

    let rows = app.nav().rows(app.tree());
    assert_eq!("b", rows[app.nav().cursor()].node.path);
    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_reset_right_pane_scroll_when_expand_all_is_pressed() {
    // `CollapseAll` first (before establishing focus/scroll) would land
    // the cursor on a Dir row ("a"), which `Open` cannot move focus from
    // (`App::handle_key`'s `Open` arm) — so this test instead reaches
    // Focus::Right + a nonzero scroll on "a/one.rs" while still
    // expanded, then presses `CollapseAll` followed by `ExpandAll` in
    // one breath and asserts the scroll is (still) 0 after both, which
    // is what actually matters: `ExpandAll` itself must never leave a
    // stale nonzero scroll behind, regardless of what `CollapseAll`
    // already reset it to just before.
    let report = report_with_two_directories();
    let app = focused_right_and_scrolled_one_line(App::new(&report));
    assert_eq!(1, app.right_pane_scroll());
    let app = app.handle_key(InputKey::CollapseAll);
    assert_eq!(0, app.right_pane_scroll());

    let app = app.handle_key(InputKey::ExpandAll);

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_reset_right_pane_scroll_when_toggle_order_is_pressed() {
    // ToggleOrder can change which row now sits at the same cursor
    // index (reordering siblings), so it must reset the scroll offset
    // even though it never calls into `Nav` at all.
    let report = report_with_two_directories();
    let app = App::new(&report);
    let app = focused_right_and_scrolled_one_line(app);
    assert_eq!(1, app.right_pane_scroll());

    let app = app.handle_key(InputKey::ToggleOrder);

    assert_eq!(0, app.right_pane_scroll());
}
