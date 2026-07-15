//! Tests for `App::handle_review_key` (ADR 0048's review-overlay key
//! dispatch): the annotations-list overlay's own binding grammar (`j`/`k`
//! move, Enter proceed, Esc back, `d` delete — unified with the jump
//! popup's grammar) and the `pending_prefix` clear when `a` (ADR 0058) is
//! pressed over a row with no derivable snapshot.

use super::*;
use crate::review::{ReviewState, SelectionSnapshot};

fn snapshot() -> SelectionSnapshot {
    SelectionSnapshot {
        path: "lib.rs".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        symbol_name: Some("foo".to_string()),
        range: Some((1, 1)),
        anchor: Some((1, 1)),
        signature: Some("fn foo()".to_string()),
    }
}

#[test]
fn should_open_export_menu_when_confirming_the_annotations_list() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list();
    let app = App::new(&report).with_review(review);

    let actual = app.handle_key(InputKey::PopupConfirm);

    assert_eq!(
        &crate::review::ReviewMode::ExportMenu { cursor: 0 },
        actual.review().mode()
    );
}
