use super::*;
use crate::review::NoteLocation;

fn note(location: NoteLocation, body: &str, signature: Option<&str>) -> Note {
    Note {
        location,
        body: body.to_string(),
        signature: signature.map(str::to_string),
    }
}

mod render_review_comments_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_omit_start_line_when_anchor_is_a_single_line() {
        let notes = vec![note(
            NoteLocation {
                path: "src/lib.rs".to_string(),
                symbol_id: Some("src/lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((10, 20)),
                anchor: Some((15, 15)),
            },
            "please rename this",
            Some("fn foo()"),
        )];

        let actual = render_review_comments(&notes);

        assert_eq!(
            vec![RenderedComment {
                path: "src/lib.rs".to_string(),
                line: 15,
                start_line: None,
                body: "please rename this".to_string(),
            }],
            actual
        );
    }

    #[test]
    fn should_set_start_line_when_anchor_spans_multiple_lines() {
        let notes = vec![note(
            NoteLocation {
                path: "src/lib.rs".to_string(),
                symbol_id: Some("src/lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((10, 20)),
                anchor: Some((12, 18)),
            },
            "this whole block needs a test",
            None,
        )];

        let actual = render_review_comments(&notes);

        assert_eq!(
            vec![RenderedComment {
                path: "src/lib.rs".to_string(),
                line: 18,
                start_line: Some(12),
                body: "this whole block needs a test".to_string(),
            }],
            actual
        );
    }

    #[test]
    fn should_fall_back_to_range_when_anchor_is_absent() {
        let notes = vec![note(
            NoteLocation {
                path: "src/lib.rs".to_string(),
                symbol_id: Some("src/lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((5, 9)),
                anchor: None,
            },
            "note without an anchor",
            None,
        )];

        let actual = render_review_comments(&notes);

        assert_eq!(
            vec![RenderedComment {
                path: "src/lib.rs".to_string(),
                line: 9,
                start_line: Some(5),
                body: "note without an anchor".to_string(),
            }],
            actual
        );
    }

    #[test]
    fn should_fall_back_to_line_one_when_neither_anchor_nor_range_is_present() {
        let notes = vec![note(
            NoteLocation {
                path: "src/lib.rs".to_string(),
                symbol_id: None,
                symbol_name: None,
                range: None,
                anchor: None,
            },
            "note on a non-symbol location",
            None,
        )];

        let actual = render_review_comments(&notes);

        assert_eq!(
            vec![RenderedComment {
                path: "src/lib.rs".to_string(),
                line: 1,
                start_line: None,
                body: "note on a non-symbol location".to_string(),
            }],
            actual
        );
    }

    #[test]
    fn should_render_one_comment_per_note_in_order() {
        let notes = vec![
            note(
                NoteLocation {
                    path: "a.rs".to_string(),
                    symbol_id: None,
                    symbol_name: None,
                    range: None,
                    anchor: Some((1, 1)),
                },
                "first",
                None,
            ),
            note(
                NoteLocation {
                    path: "b.rs".to_string(),
                    symbol_id: None,
                    symbol_name: None,
                    range: None,
                    anchor: Some((2, 2)),
                },
                "second",
                None,
            ),
        ];

        let actual = render_review_comments(&notes);

        assert_eq!(
            vec![
                RenderedComment {
                    path: "a.rs".to_string(),
                    line: 1,
                    start_line: None,
                    body: "first".to_string(),
                },
                RenderedComment {
                    path: "b.rs".to_string(),
                    line: 2,
                    start_line: None,
                    body: "second".to_string(),
                },
            ],
            actual
        );
    }
}

mod render_agent_packet_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_render_empty_packet_header_when_there_are_no_notes() {
        let actual = render_agent_packet(&[]);

        assert_eq!(
            "# Review notes\n\nAddress each of the following review notes.\n",
            actual
        );
    }

    #[test]
    fn should_render_heading_signature_and_body_for_a_symbol_note() {
        let notes = vec![note(
            NoteLocation {
                path: "src/lib.rs".to_string(),
                symbol_id: Some("src/lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((10, 20)),
                anchor: Some((12, 18)),
            },
            "please add a doc comment",
            Some("fn foo(x: i32) -> i32"),
        )];

        let actual = render_agent_packet(&notes);

        assert_eq!(
            "# Review notes\n\n\
             Address each of the following review notes.\n\n\
             ## src/lib.rs:12-18 foo\n\
             ```\n\
             fn foo(x: i32) -> i32\n\
             ```\n\
             please add a doc comment\n",
            actual
        );
    }

    #[test]
    fn should_render_single_line_range_without_a_dash() {
        let notes = vec![note(
            NoteLocation {
                path: "src/lib.rs".to_string(),
                symbol_id: Some("src/lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((15, 15)),
                anchor: Some((15, 15)),
            },
            "one-line note",
            None,
        )];

        let actual = render_agent_packet(&notes);

        assert_eq!(
            "# Review notes\n\n\
             Address each of the following review notes.\n\n\
             ## src/lib.rs:15 foo\n\
             one-line note\n",
            actual
        );
    }

    #[test]
    fn should_render_bare_path_heading_when_location_has_no_range_or_name() {
        let notes = vec![note(
            NoteLocation {
                path: "src/lib.rs".to_string(),
                symbol_id: None,
                symbol_name: None,
                range: None,
                anchor: None,
            },
            "note without location detail",
            None,
        )];

        let actual = render_agent_packet(&notes);

        assert_eq!(
            "# Review notes\n\n\
             Address each of the following review notes.\n\n\
             ## src/lib.rs\n\
             note without location detail\n",
            actual
        );
    }

    #[test]
    fn should_render_multiple_notes_in_order() {
        let notes = vec![
            note(
                NoteLocation {
                    path: "a.rs".to_string(),
                    symbol_id: None,
                    symbol_name: Some("alpha".to_string()),
                    range: Some((1, 1)),
                    anchor: Some((1, 1)),
                },
                "first note",
                None,
            ),
            note(
                NoteLocation {
                    path: "b.rs".to_string(),
                    symbol_id: None,
                    symbol_name: Some("beta".to_string()),
                    range: Some((2, 2)),
                    anchor: Some((2, 2)),
                },
                "second note",
                None,
            ),
        ];

        let actual = render_agent_packet(&notes);

        assert_eq!(
            "# Review notes\n\n\
             Address each of the following review notes.\n\n\
             ## a.rs:1 alpha\n\
             first note\n\
             \n\
             ## b.rs:2 beta\n\
             second note\n",
            actual
        );
    }
}
