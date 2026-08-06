use super::*;
use crate::review::{AnnotationLocation, AnnotationTarget};

fn annotation(location: AnnotationLocation, body: &str, signature: Option<&str>) -> Annotation {
    Annotation {
        location,
        body: body.to_string(),
        signature: signature.map(str::to_string),
    }
}

fn symbol_location(
    path: &str,
    symbol_name: Option<&str>,
    range: Option<(usize, usize)>,
    anchor: Option<(usize, usize)>,
) -> AnnotationLocation {
    AnnotationLocation {
        target: AnnotationTarget::Symbol,
        path: path.to_string(),
        symbol_id: symbol_name.map(|name| format!("{path}::{name}")),
        symbol_name: symbol_name.map(str::to_string),
        range,
        anchor,
    }
}

fn file_location(path: &str) -> AnnotationLocation {
    AnnotationLocation {
        target: AnnotationTarget::File,
        path: path.to_string(),
        symbol_id: None,
        symbol_name: None,
        range: None,
        anchor: None,
    }
}

fn dir_location(path: &str) -> AnnotationLocation {
    AnnotationLocation {
        target: AnnotationTarget::Dir,
        path: path.to_string(),
        symbol_id: None,
        symbol_name: None,
        range: None,
        anchor: None,
    }
}

fn removed_symbol_location(path: &str, name: &str) -> AnnotationLocation {
    AnnotationLocation {
        target: AnnotationTarget::RemovedSymbol,
        path: path.to_string(),
        symbol_id: Some(format!("{path}::{name}")),
        symbol_name: Some(name.to_string()),
        range: None,
        anchor: None,
    }
}

mod partition_for_export_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_split_annotations_by_whether_an_anchor_or_range_resolved() {
        let anchored = annotation(
            symbol_location("lib.rs", Some("foo"), Some((10, 20)), Some((15, 15))),
            "anchored",
            None,
        );
        let unanchored_file = annotation(file_location("lib.rs"), "file note", None);
        let annotations = vec![anchored.clone(), unanchored_file.clone()];

        let (actual_anchored, actual_unanchored) = partition_for_export(&annotations);

        assert_eq!(vec![&anchored], actual_anchored);
        assert_eq!(vec![&unanchored_file], actual_unanchored);
    }

    #[test]
    fn should_treat_a_symbol_annotation_with_no_anchor_or_range_as_unanchored() {
        let annotation = annotation(
            symbol_location("lib.rs", Some("foo"), None, None),
            "no anchor",
            None,
        );
        let annotations = vec![annotation.clone()];

        let (actual_anchored, actual_unanchored) = partition_for_export(&annotations);

        assert_eq!(Vec::<&Annotation>::new(), actual_anchored);
        assert_eq!(vec![&annotation], actual_unanchored);
    }
}

mod render_review_comments_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_omit_start_line_when_anchor_is_a_single_line() {
        let annotation = annotation(
            symbol_location("src/lib.rs", Some("foo"), Some((10, 20)), Some((15, 15))),
            "please rename this",
            Some("fn foo()"),
        );
        let annotations = vec![&annotation];

        let actual = render_review_comments(&annotations);

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
        let annotation = annotation(
            symbol_location("src/lib.rs", Some("foo"), Some((10, 20)), Some((12, 18))),
            "this whole block needs a test",
            None,
        );
        let annotations = vec![&annotation];

        let actual = render_review_comments(&annotations);

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
        let annotation = annotation(
            symbol_location("src/lib.rs", Some("foo"), Some((5, 9)), None),
            "annotation without an anchor",
            None,
        );
        let annotations = vec![&annotation];

        let actual = render_review_comments(&annotations);

        assert_eq!(
            vec![RenderedComment {
                path: "src/lib.rs".to_string(),
                line: 9,
                start_line: Some(5),
                body: "annotation without an anchor".to_string(),
            }],
            actual
        );
    }

    #[test]
    fn should_omit_a_comment_when_neither_anchor_nor_range_is_present() {
        // partition_for_export is what should keep an unanchored annotation
        // from reaching render_review_comments in production use (ADR
        // 0067); this test pins render_review_comments' own defensive
        // behavior if that invariant is ever violated directly.
        let annotation = annotation(file_location("src/lib.rs"), "file-level note", None);
        let annotations = vec![&annotation];

        let actual = render_review_comments(&annotations);

        assert_eq!(Vec::<RenderedComment>::new(), actual);
    }

    #[test]
    fn should_render_one_comment_per_annotation_in_order() {
        let first = annotation(
            symbol_location("a.rs", None, None, Some((1, 1))),
            "first",
            None,
        );
        let second = annotation(
            symbol_location("b.rs", None, None, Some((2, 2))),
            "second",
            None,
        );
        let annotations = vec![&first, &second];

        let actual = render_review_comments(&annotations);

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

mod render_additional_notes_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_render_empty_string_when_there_are_no_unanchored_annotations() {
        let actual = render_additional_notes(&[]);

        assert_eq!(String::new(), actual);
    }

    #[test]
    fn should_render_one_bullet_per_annotation_with_its_first_body_line() {
        let file_note = annotation(file_location("src/lib.rs"), "dead code now", None);
        let dir_note = annotation(
            dir_location("src/legacy"),
            "structure is confusing\nsecond line ignored",
            None,
        );
        let removed_note = annotation(
            removed_symbol_location("src/lib.rs", "old_helper"),
            "should not have been removed",
            None,
        );
        let annotations = vec![&file_note, &dir_note, &removed_note];

        let actual = render_additional_notes(&annotations);

        assert_eq!(
            "\n\n## Additional notes\n\
             - `src/lib.rs`: dead code now\n\
             - `src/legacy/`: structure is confusing\n\
             - `src/lib.rs old_helper (removed)`: should not have been removed\n",
            actual
        );
    }
}

mod render_agent_packet_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_render_empty_packet_header_when_there_are_no_annotations() {
        let actual = render_agent_packet(&[]);

        assert_eq!(
            "# Review annotations\n\nAddress each of the following review annotations.\n",
            actual
        );
    }

    #[test]
    fn should_render_heading_signature_and_body_for_a_symbol_annotation() {
        let annotations = vec![annotation(
            symbol_location("src/lib.rs", Some("foo"), Some((10, 20)), Some((12, 18))),
            "please add a doc comment",
            Some("fn foo(x: i32) -> i32"),
        )];

        let actual = render_agent_packet(&annotations);

        assert_eq!(
            "# Review annotations\n\n\
             Address each of the following review annotations.\n\n\
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
        let annotations = vec![annotation(
            symbol_location("src/lib.rs", Some("foo"), Some((15, 15)), Some((15, 15))),
            "one-line annotation",
            None,
        )];

        let actual = render_agent_packet(&annotations);

        assert_eq!(
            "# Review annotations\n\n\
             Address each of the following review annotations.\n\n\
             ## src/lib.rs:15 foo\n\
             one-line annotation\n",
            actual
        );
    }

    #[test]
    fn should_render_bare_path_heading_for_a_file_annotation() {
        let annotations = vec![annotation(
            file_location("src/lib.rs"),
            "this whole file is dead code now",
            None,
        )];

        let actual = render_agent_packet(&annotations);

        assert_eq!(
            "# Review annotations\n\n\
             Address each of the following review annotations.\n\n\
             ## src/lib.rs\n\
             this whole file is dead code now\n",
            actual
        );
    }

    #[test]
    fn should_render_trailing_slash_heading_for_a_dir_annotation() {
        let annotations = vec![annotation(
            dir_location("src/legacy"),
            "this directory's structure is confusing",
            None,
        )];

        let actual = render_agent_packet(&annotations);

        assert_eq!(
            "# Review annotations\n\n\
             Address each of the following review annotations.\n\n\
             ## src/legacy/\n\
             this directory's structure is confusing\n",
            actual
        );
    }

    #[test]
    fn should_render_path_and_name_heading_for_a_removed_symbol_annotation() {
        let annotations = vec![annotation(
            removed_symbol_location("src/lib.rs", "old_helper"),
            "this should not have been removed",
            None,
        )];

        let actual = render_agent_packet(&annotations);

        assert_eq!(
            "# Review annotations\n\n\
             Address each of the following review annotations.\n\n\
             ## src/lib.rs old_helper (removed)\n\
             this should not have been removed\n",
            actual
        );
    }

    #[test]
    fn should_render_multiple_annotations_in_order() {
        let annotations = vec![
            annotation(
                symbol_location("a.rs", Some("alpha"), Some((1, 1)), Some((1, 1))),
                "first annotation",
                None,
            ),
            annotation(
                symbol_location("b.rs", Some("beta"), Some((2, 2)), Some((2, 2))),
                "second annotation",
                None,
            ),
        ];

        let actual = render_agent_packet(&annotations);

        assert_eq!(
            "# Review annotations\n\n\
             Address each of the following review annotations.\n\n\
             ## a.rs:1 alpha\n\
             first annotation\n\
             \n\
             ## b.rs:2 beta\n\
             second annotation\n",
            actual
        );
    }
}
