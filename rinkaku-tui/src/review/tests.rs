use super::*;

fn snapshot(path: &str) -> SelectionSnapshot {
    SelectionSnapshot {
        path: path.to_string(),
        symbol_id: Some(format!("{path}::foo")),
        symbol_name: Some("foo".to_string()),
        range: Some((1, 5)),
        anchor: Some((1, 5)),
        signature: Some("fn foo()".to_string()),
    }
}

mod compose_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_open_compose_mode_with_empty_buffer_when_beginning_compose() {
        let state = ReviewState::default().begin_compose(snapshot("lib.rs"));

        assert_eq!(
            &ReviewMode::Compose {
                snapshot: snapshot("lib.rs"),
                buffer: String::new(),
            },
            state.mode()
        );
    }

    #[test]
    fn should_append_typed_characters_to_the_compose_buffer() {
        let state = ReviewState::default()
            .begin_compose(snapshot("lib.rs"))
            .push_char('h')
            .push_char('i');

        assert_eq!(
            &ReviewMode::Compose {
                snapshot: snapshot("lib.rs"),
                buffer: "hi".to_string(),
            },
            state.mode()
        );
    }

    #[test]
    fn should_remove_last_character_on_backspace() {
        let state = ReviewState::default()
            .begin_compose(snapshot("lib.rs"))
            .push_char('h')
            .push_char('i')
            .backspace();

        assert_eq!(
            &ReviewMode::Compose {
                snapshot: snapshot("lib.rs"),
                buffer: "h".to_string(),
            },
            state.mode()
        );
    }

    #[test]
    fn should_be_a_no_op_when_backspacing_an_empty_buffer() {
        let state = ReviewState::default()
            .begin_compose(snapshot("lib.rs"))
            .backspace();

        assert_eq!(
            &ReviewMode::Compose {
                snapshot: snapshot("lib.rs"),
                buffer: String::new(),
            },
            state.mode()
        );
    }

    #[test]
    fn should_add_an_annotation_and_return_to_idle_when_confirming_a_non_blank_buffer() {
        let state = ReviewState::default()
            .begin_compose(snapshot("lib.rs"))
            .push_char('h')
            .push_char('i')
            .confirm_compose();

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert_eq!(
            &[Annotation {
                location: AnnotationLocation::from(snapshot("lib.rs")),
                body: "hi".to_string(),
                signature: Some("fn foo()".to_string()),
            }],
            state.annotations()
        );
        assert_eq!(1, state.revision());
    }

    #[test]
    fn should_not_add_an_annotation_when_confirming_an_empty_buffer() {
        let state = ReviewState::default()
            .begin_compose(snapshot("lib.rs"))
            .confirm_compose();

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert!(state.annotations().is_empty());
        assert_eq!(0, state.revision());
    }

    #[test]
    fn should_not_add_an_annotation_when_confirming_a_whitespace_only_buffer() {
        let state = ReviewState::default()
            .begin_compose(snapshot("lib.rs"))
            .push_char(' ')
            .push_char(' ')
            .confirm_compose();

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert!(state.annotations().is_empty());
        assert_eq!(0, state.revision());
    }

    #[test]
    fn should_discard_the_buffer_and_return_to_idle_when_cancelling_compose() {
        let state = ReviewState::default()
            .begin_compose(snapshot("lib.rs"))
            .push_char('h')
            .cancel_compose();

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert!(state.annotations().is_empty());
        assert_eq!(0, state.revision());
    }

    #[test]
    fn should_be_a_no_op_when_confirming_compose_outside_compose_mode() {
        let state = ReviewState::default().confirm_compose();

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert!(state.annotations().is_empty());
    }

    #[test]
    fn should_be_a_no_op_when_cancelling_compose_outside_compose_mode() {
        let state = ReviewState::default().cancel_compose();

        assert_eq!(&ReviewMode::Idle, state.mode());
    }
}

mod list_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn state_with_two_annotations() -> ReviewState {
        ReviewState::default()
            .begin_compose(snapshot("a.rs"))
            .push_char('a')
            .confirm_compose()
            .begin_compose(snapshot("b.rs"))
            .push_char('b')
            .confirm_compose()
    }

    #[test]
    fn should_open_list_mode_with_cursor_on_first_annotation() {
        let state = state_with_two_annotations().open_list();

        assert_eq!(&ReviewMode::List { cursor: 0 }, state.mode());
    }

    #[test]
    fn should_move_list_cursor_down_clamped_to_the_last_annotation() {
        let state = state_with_two_annotations()
            .open_list()
            .list_down()
            .list_down();

        assert_eq!(&ReviewMode::List { cursor: 1 }, state.mode());
    }

    #[test]
    fn should_move_list_cursor_up_clamped_to_zero() {
        let state = state_with_two_annotations().open_list().list_up();

        assert_eq!(&ReviewMode::List { cursor: 0 }, state.mode());
    }

    #[test]
    fn should_close_the_list_and_return_to_idle() {
        let state = state_with_two_annotations().open_list().close();

        assert_eq!(&ReviewMode::Idle, state.mode());
    }

    #[test]
    fn should_delete_the_annotation_under_the_list_cursor() {
        let state = state_with_two_annotations()
            .open_list()
            .list_down()
            .delete_selected();

        assert_eq!(
            &[Annotation {
                location: AnnotationLocation::from(snapshot("a.rs")),
                body: "a".to_string(),
                signature: Some("fn foo()".to_string()),
            }],
            state.annotations()
        );
        assert_eq!(3, state.revision());
    }

    #[test]
    fn should_clamp_list_cursor_after_deleting_the_last_annotation() {
        let state = state_with_two_annotations()
            .open_list()
            .list_down()
            .delete_selected();

        assert_eq!(&ReviewMode::List { cursor: 0 }, state.mode());
    }

    #[test]
    fn should_be_a_no_op_when_deleting_from_an_empty_list() {
        let state = ReviewState::default().open_list().delete_selected();

        assert_eq!(&ReviewMode::List { cursor: 0 }, state.mode());
        assert!(state.annotations().is_empty());
        assert_eq!(0, state.revision());
    }
}

mod export_menu_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_be_a_no_op_when_opening_export_menu_outside_list_mode() {
        let state = ReviewState::default().open_export_menu();

        assert_eq!(&ReviewMode::Idle, state.mode());
    }

    #[test]
    fn should_open_export_menu_from_list_mode() {
        let state = ReviewState::default().open_list().open_export_menu();

        assert_eq!(&ReviewMode::ExportMenu { cursor: 0 }, state.mode());
    }

    #[test]
    fn should_open_verdict_menu_when_confirming_github_entry_with_sink_a_available() {
        let state = ReviewState::default()
            .open_list()
            .open_export_menu()
            .confirm_export(true);

        assert_eq!(&ReviewMode::VerdictMenu { cursor: 0 }, state.mode());
    }

    #[test]
    fn should_set_clipboard_pending_export_when_sink_a_is_unavailable_and_first_entry_is_confirmed()
    {
        // With sink A unavailable, the menu's only entry (index 0) is
        // Clipboard — mirrors the "sink A is absent, not disabled" rule
        // (ADR 0048): there is no Github entry to skip past.
        let mut state = ReviewState::default()
            .open_list()
            .open_export_menu()
            .confirm_export(false);

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert_eq!(Some(ExportRequest::Clipboard), state.take_pending_export());
    }

    #[test]
    fn should_set_clipboard_pending_export_when_sink_a_is_available_and_second_entry_is_confirmed()
    {
        let mut state = ReviewState::default()
            .open_list()
            .open_export_menu()
            .list_down()
            .confirm_export(true);

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert_eq!(Some(ExportRequest::Clipboard), state.take_pending_export());
    }

    #[test]
    fn should_close_menu_without_pending_export_when_cursor_overshoots_available_entries() {
        // `list_down`'s clamp always assumes both entries exist (module
        // doc comment on that method) — with sink A unavailable, cursor 1
        // is out of range for the single-entry menu `confirm_export(false)`
        // resolves against, so this pins the fallback path stays a no-op
        // export rather than panicking or misfiring Clipboard.
        let mut state = ReviewState::default()
            .open_list()
            .open_export_menu()
            .list_down()
            .confirm_export(false);

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert_eq!(None, state.take_pending_export());
    }

    #[test]
    fn should_be_a_no_op_when_confirming_export_outside_export_menu_mode() {
        let mut state = ReviewState::default().confirm_export(true);

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert_eq!(None, state.take_pending_export());
    }
}

mod verdict_menu_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn at_verdict_menu() -> ReviewState {
        ReviewState::default()
            .open_list()
            .open_export_menu()
            .confirm_export(true)
    }

    #[test]
    fn should_set_approve_pending_export_when_confirming_first_entry() {
        let mut state = at_verdict_menu().confirm_verdict();

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert_eq!(
            Some(ExportRequest::GithubReview(Verdict::Approve)),
            state.take_pending_export()
        );
    }

    #[test]
    fn should_set_request_changes_pending_export_when_confirming_second_entry() {
        let mut state = at_verdict_menu().list_down().confirm_verdict();

        assert_eq!(
            Some(ExportRequest::GithubReview(Verdict::RequestChanges)),
            state.take_pending_export()
        );
    }

    #[test]
    fn should_set_comment_pending_export_when_confirming_third_entry() {
        let mut state = at_verdict_menu().list_down().list_down().confirm_verdict();

        assert_eq!(
            Some(ExportRequest::GithubReview(Verdict::Comment)),
            state.take_pending_export()
        );
    }

    #[test]
    fn should_clamp_verdict_cursor_to_the_last_entry() {
        let state = at_verdict_menu().list_down().list_down().list_down();

        assert_eq!(&ReviewMode::VerdictMenu { cursor: 2 }, state.mode());
    }

    #[test]
    fn should_be_a_no_op_when_confirming_verdict_outside_verdict_menu_mode() {
        let mut state = ReviewState::default().confirm_verdict();

        assert_eq!(&ReviewMode::Idle, state.mode());
        assert_eq!(None, state.take_pending_export());
    }
}

mod status_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_store_the_status_message() {
        let state = ReviewState::default().set_status("posted review");

        assert_eq!(Some("posted review"), state.last_status());
    }
}

mod revision_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_not_change_revision_when_opening_or_closing_overlays() {
        let state = ReviewState::default().open_list().close().open_list();

        assert_eq!(0, state.revision());
    }
}

mod pr_url_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_build_the_pr_page_url_from_owner_repo_and_number() {
        let ctx = PrContext {
            owner: "hiro-o918".to_string(),
            repo: "rinkaku".to_string(),
            number: 42,
            head_sha: "deadbeef".to_string(),
        };

        let actual = pr_url(&ctx);

        assert_eq!("https://github.com/hiro-o918/rinkaku/pull/42", actual);
    }
}
