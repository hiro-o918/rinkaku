use super::*;
use crate::review::NoteLocation;

fn note(path: &str, symbol_id: Option<&str>, range: Option<(usize, usize)>) -> Note {
    Note {
        location: NoteLocation {
            path: path.to_string(),
            symbol_id: symbol_id.map(str::to_string),
            symbol_name: None,
            range,
            anchor: None,
        },
        body: "note body".to_string(),
        signature: None,
    }
}

mod build_note_markers_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_empty_markers_when_there_are_no_notes() {
        let actual = build_note_markers(&[]);

        assert_eq!(NoteMarkers::default(), actual);
    }

    #[test]
    fn should_count_notes_per_symbol_and_per_file() {
        let notes = vec![
            note("lib.rs", Some("lib.rs::foo"), Some((1, 5))),
            note("lib.rs", Some("lib.rs::foo"), Some((2, 3))),
            note("lib.rs", Some("lib.rs::bar"), Some((10, 12))),
        ];

        let actual = build_note_markers(&notes);

        assert_eq!(
            NoteMarkers {
                symbol_counts: HashMap::from([
                    ("lib.rs::foo".to_string(), 2),
                    ("lib.rs::bar".to_string(), 1),
                ]),
                file_counts: HashMap::from([("lib.rs".to_string(), 3)]),
                line_ranges: HashMap::from([(
                    "lib.rs".to_string(),
                    vec![(1, 5), (2, 3), (10, 12)]
                )]),
            },
            actual
        );
    }

    #[test]
    fn should_count_file_note_without_incrementing_symbol_counts_when_symbol_id_is_absent() {
        let notes = vec![note("lib.rs", None, Some((1, 1)))];

        let actual = build_note_markers(&notes);

        assert_eq!(
            NoteMarkers {
                symbol_counts: HashMap::new(),
                file_counts: HashMap::from([("lib.rs".to_string(), 1)]),
                line_ranges: HashMap::from([("lib.rs".to_string(), vec![(1, 1)])]),
            },
            actual
        );
    }

    #[test]
    fn should_omit_line_ranges_entry_when_note_has_no_range() {
        let notes = vec![note("lib.rs", Some("lib.rs::foo"), None)];

        let actual = build_note_markers(&notes);

        assert_eq!(
            NoteMarkers {
                symbol_counts: HashMap::from([("lib.rs::foo".to_string(), 1)]),
                file_counts: HashMap::from([("lib.rs".to_string(), 1)]),
                line_ranges: HashMap::new(),
            },
            actual
        );
    }
}

mod line_has_note_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_true_when_line_falls_inside_a_note_range() {
        let markers = build_note_markers(&[note("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(true, line_has_note(&markers, "lib.rs", 7));
    }

    #[test]
    fn should_return_true_when_line_is_a_range_boundary() {
        let markers = build_note_markers(&[note("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(true, line_has_note(&markers, "lib.rs", 5));
        assert_eq!(true, line_has_note(&markers, "lib.rs", 10));
    }

    #[test]
    fn should_return_false_when_line_falls_outside_every_note_range() {
        let markers = build_note_markers(&[note("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(false, line_has_note(&markers, "lib.rs", 11));
    }

    #[test]
    fn should_return_false_when_path_has_no_notes_at_all() {
        let markers = build_note_markers(&[note("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(false, line_has_note(&markers, "other.rs", 7));
    }
}
