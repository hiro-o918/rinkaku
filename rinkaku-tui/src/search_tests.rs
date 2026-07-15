use super::*;
use pretty_assertions::assert_eq;
use rstest::rstest;

// --- is_case_sensitive / smartcase_matches_line ---

#[rstest]
#[case::should_be_insensitive_when_query_is_all_lowercase("foo", false)]
#[case::should_be_sensitive_when_query_has_any_uppercase("Foo", true)]
#[case::should_be_sensitive_when_query_is_all_uppercase("FOO", true)]
#[case::should_be_insensitive_when_query_has_no_letters("123", false)]
fn should_detect_case_sensitivity_per_smartcase_rule(#[case] query: &str, #[case] expected: bool) {
    let actual = is_case_sensitive(query);
    assert_eq!(expected, actual);
}

#[rstest]
#[case::should_match_case_insensitively_when_query_is_lowercase("let Foo = 1;", "foo", true)]
#[case::should_match_case_sensitively_when_query_has_uppercase("let Foo = 1;", "Foo", true)]
#[case::should_not_match_case_sensitively_when_case_differs("let foo = 1;", "Foo", false)]
#[case::should_match_plain_substring("hello world", "wor", true)]
#[case::should_not_match_when_substring_absent("hello world", "xyz", false)]
fn should_apply_smartcase_when_matching_a_line(
    #[case] line: &str,
    #[case] query: &str,
    #[case] expected: bool,
) {
    let actual = smartcase_matches_line(line, query);
    assert_eq!(expected, actual);
}

// --- find_matches ---

#[test]
fn should_return_empty_matches_when_query_is_empty() {
    let lines = vec!["foo".to_string(), "bar".to_string()];

    let actual = find_matches(&lines, "");

    assert_eq!(Vec::<MatchLine>::new(), actual);
}

#[test]
fn should_return_no_matches_when_query_is_absent_from_every_line() {
    let lines = vec!["foo".to_string(), "bar".to_string()];

    let actual = find_matches(&lines, "xyz");

    assert_eq!(Vec::<MatchLine>::new(), actual);
}

#[test]
fn should_return_every_matching_line_index_in_ascending_order() {
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];

    let actual = find_matches(&lines, "foo");

    assert_eq!(vec![0usize, 2usize], actual);
}

#[test]
fn should_respect_smartcase_when_finding_matches() {
    let lines = vec!["let Foo = 1;".to_string(), "let bar = 2;".to_string()];

    let actual = find_matches(&lines, "Foo");

    assert_eq!(vec![0usize], actual);
}

// --- next_match_index / prev_match_index ---

#[rstest]
#[case::should_advance_to_next_index(3, 0, Some(1))]
#[case::should_wrap_to_first_from_last(3, 2, Some(0))]
#[case::should_return_none_when_no_matches(0, 0, None)]
fn should_compute_next_match_index(
    #[case] total: usize,
    #[case] current: usize,
    #[case] expected: Option<usize>,
) {
    let actual = next_match_index(total, current);
    assert_eq!(expected, actual);
}

#[rstest]
#[case::should_retreat_to_previous_index(3, 1, Some(0))]
#[case::should_wrap_to_last_from_first(3, 0, Some(2))]
#[case::should_return_none_when_no_matches(0, 0, None)]
fn should_compute_prev_match_index(
    #[case] total: usize,
    #[case] current: usize,
    #[case] expected: Option<usize>,
) {
    let actual = prev_match_index(total, current);
    assert_eq!(expected, actual);
}

// --- SearchState transitions ---

#[test]
fn should_start_composing_with_an_empty_buffer_when_inactive() {
    let state = SearchState::default().start();

    assert_eq!(
        &SearchMode::Composing {
            buffer: String::new()
        },
        state.mode()
    );
}

#[test]
fn should_not_restart_the_buffer_when_start_is_pressed_while_already_composing() {
    let state = SearchState::default().start().push_char('a').start();

    assert_eq!(
        &SearchMode::Composing {
            buffer: "a".to_string()
        },
        state.mode()
    );
}

#[test]
fn should_build_up_the_composing_buffer_via_push_char() {
    let state = SearchState::default().start().push_char('f').push_char('o');

    assert_eq!(
        &SearchMode::Composing {
            buffer: "fo".to_string()
        },
        state.mode()
    );
}

#[test]
fn should_ignore_push_char_when_not_composing() {
    let state = SearchState::default().push_char('a');

    assert_eq!(&SearchMode::Inactive, state.mode());
}

#[test]
fn should_remove_the_last_character_via_backspace() {
    let state = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .backspace();

    assert_eq!(
        &SearchMode::Composing {
            buffer: "f".to_string()
        },
        state.mode()
    );
}

#[test]
fn should_ignore_backspace_on_an_already_empty_buffer() {
    let state = SearchState::default().start().backspace();

    assert_eq!(
        &SearchMode::Composing {
            buffer: String::new()
        },
        state.mode()
    );
}

#[test]
fn should_confirm_a_query_and_compute_its_matches() {
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];
    let state = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 0);

    assert_eq!(&SearchMode::Inactive, state.mode());
    assert_eq!(Some("foo"), state.query());
    assert_eq!(&[0usize, 2usize], state.matches());
    assert_eq!(Some(0), state.current_match());
    assert_eq!(Some((1, 2)), state.match_position());
}

#[test]
fn should_jump_to_the_first_match_at_or_after_from_line_when_confirming() {
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];
    let state = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 1);

    assert_eq!(Some(2), state.current_match());
    assert_eq!(Some((2, 2)), state.match_position());
}

#[test]
fn should_wrap_to_the_first_match_when_from_line_is_past_every_match() {
    let lines = vec!["fn foo() {}".to_string(), "fn bar() {}".to_string()];
    let state = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 1);

    assert_eq!(Some(0), state.current_match());
}

#[test]
fn should_report_zero_match_position_when_confirmed_query_has_no_matches() {
    let lines = vec!["fn bar() {}".to_string()];
    let state = SearchState::default()
        .start()
        .push_char('x')
        .push_char('y')
        .push_char('z')
        .confirm(&lines, 0);

    assert_eq!(Some("xyz"), state.query());
    assert_eq!(Some((0, 0)), state.match_position());
    assert_eq!(None, state.current_match());
}

#[test]
fn should_report_no_match_position_when_no_query_was_ever_confirmed() {
    let state = SearchState::default();

    assert_eq!(None, state.match_position());
}

#[test]
fn should_cancel_composing_instead_of_confirming_an_empty_buffer() {
    let lines = vec!["fn foo() {}".to_string()];
    let state = SearchState::default().start().confirm(&lines, 0);

    assert_eq!(&SearchMode::Inactive, state.mode());
    assert_eq!(None, state.query());
}

#[test]
fn should_cancel_composing_instead_of_confirming_a_whitespace_only_buffer() {
    let lines = vec!["fn foo() {}".to_string()];
    let state = SearchState::default()
        .start()
        .push_char(' ')
        .confirm(&lines, 0);

    assert_eq!(&SearchMode::Inactive, state.mode());
    assert_eq!(None, state.query());
}

#[test]
fn should_ignore_confirm_when_not_composing() {
    let lines = vec!["fn foo() {}".to_string()];
    let state = SearchState::default().confirm(&lines, 0);

    assert_eq!(&SearchMode::Inactive, state.mode());
    assert_eq!(None, state.query());
}

#[test]
fn should_clear_everything_on_cancel_after_a_confirmed_search() {
    let lines = vec!["fn foo() {}".to_string()];
    let state = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 0)
        .cancel();

    assert_eq!(&SearchMode::Inactive, state.mode());
    assert_eq!(None, state.query());
    assert_eq!(&[] as &[MatchLine], state.matches());
    assert_eq!(None, state.match_position());
}

#[test]
fn should_advance_to_the_next_match_and_wrap() {
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];
    let mut state = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 0);
    assert_eq!(Some(0), state.current_match());

    state = state.next();
    assert_eq!(Some(2), state.current_match());

    state = state.next();
    assert_eq!(Some(0), state.current_match());
}

#[test]
fn should_retreat_to_the_previous_match_and_wrap() {
    let lines = vec![
        "fn foo() {}".to_string(),
        "fn bar() {}".to_string(),
        "fn foo_helper() {}".to_string(),
    ];
    let mut state = SearchState::default()
        .start()
        .push_char('f')
        .push_char('o')
        .push_char('o')
        .confirm(&lines, 0);
    assert_eq!(Some(0), state.current_match());

    state = state.prev();
    assert_eq!(Some(2), state.current_match());
}

#[test]
fn should_ignore_next_and_prev_when_there_are_no_confirmed_matches() {
    let state = SearchState::default();

    let after_next = state.clone().next();
    let after_prev = state.prev();

    assert_eq!(SearchState::default(), after_next);
    assert_eq!(SearchState::default(), after_prev);
}
