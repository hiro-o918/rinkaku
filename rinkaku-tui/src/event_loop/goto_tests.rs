//! `resolve_goto` tests (ADR 0022): the 0/1/many candidate resolution for
//! `gd`/`gr`, extracted so it is unit-testable without a live terminal
//! (`resolve_goto`'s own doc comment). The `run_app`-equivalent dispatch
//! sequence around it (`pending_prefix` clear + jumplist scroll-restore)
//! lives in `crate::event_loop`'s own test tree instead, since it pins
//! `dispatch_non_source_key`, not `resolve_goto` itself.

use super::{GotoOutcome, resolve_goto};
use crate::app::{App, InputKey};
use crate::event_loop::tests::report_with_symbols_and_edges;
use pretty_assertions::assert_eq;

fn candidate(id: &str, name: &str, path: &str) -> crate::app::JumpCandidate {
    crate::app::JumpCandidate {
        id: id.to_string(),
        name: name.to_string(),
        path: path.to_string(),
    }
}

#[test]
fn should_return_no_symbol_selected_when_cursor_is_not_on_a_symbol_row() {
    let report = report_with_symbols_and_edges(vec![("lib.rs", vec!["foo"])], vec![]);
    let app = App::new(&report); // cursor on "lib.rs" (a File row)

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(GotoOutcome::NoSymbolSelected, actual);
}

#[test]
fn should_return_no_candidates_when_selected_symbol_has_no_callees() {
    let report = report_with_symbols_and_edges(vec![("lib.rs", vec!["foo"])], vec![]);
    let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(GotoOutcome::NoCandidates("callees"), actual);
}

#[test]
fn should_return_one_candidate_when_selected_symbol_has_exactly_one_callee() {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar"])],
        vec![("lib.rs::foo", "lib.rs::bar")],
    );
    let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(
        GotoOutcome::One(candidate("lib.rs::bar", "bar", "lib.rs")),
        actual
    );
}

#[test]
fn should_return_many_candidates_when_selected_symbol_has_multiple_callees() {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar", "baz"])],
        vec![
            ("lib.rs::foo", "lib.rs::bar"),
            ("lib.rs::foo", "lib.rs::baz"),
        ],
    );
    let app = App::new(&report).handle_key(InputKey::Down); // cursor on "foo"

    let actual = resolve_goto(&app, &report, InputKey::GotoDefinition);

    assert_eq!(
        GotoOutcome::Many(vec![
            candidate("lib.rs::bar", "bar", "lib.rs"),
            candidate("lib.rs::baz", "baz", "lib.rs"),
        ]),
        actual
    );
}

#[test]
fn should_resolve_callers_direction_when_goto_references_is_requested() {
    let report = report_with_symbols_and_edges(
        vec![("lib.rs", vec!["foo", "bar"])],
        vec![("lib.rs::foo", "lib.rs::bar")],
    );
    // Cursor on "bar" (row 2): "foo" is its one caller.
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);

    let actual = resolve_goto(&app, &report, InputKey::GotoReferences);

    assert_eq!(
        GotoOutcome::One(candidate("lib.rs::foo", "foo", "lib.rs")),
        actual
    );
}
