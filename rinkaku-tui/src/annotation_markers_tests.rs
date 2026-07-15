use super::*;
use crate::review::AnnotationLocation;

fn annotation(path: &str, symbol_id: Option<&str>, range: Option<(usize, usize)>) -> Annotation {
    Annotation {
        location: AnnotationLocation {
            path: path.to_string(),
            symbol_id: symbol_id.map(str::to_string),
            symbol_name: None,
            range,
            anchor: None,
        },
        body: "annotation body".to_string(),
        signature: None,
    }
}

mod build_annotation_markers_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_empty_markers_when_there_are_no_annotations() {
        let actual = build_annotation_markers(&[]);

        assert_eq!(AnnotationMarkers::default(), actual);
    }

    #[test]
    fn should_count_annotations_per_symbol_and_per_file() {
        let annotations = vec![
            annotation("lib.rs", Some("lib.rs::foo"), Some((1, 5))),
            annotation("lib.rs", Some("lib.rs::foo"), Some((2, 3))),
            annotation("lib.rs", Some("lib.rs::bar"), Some((10, 12))),
        ];

        let actual = build_annotation_markers(&annotations);

        assert_eq!(
            AnnotationMarkers {
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
    fn should_count_file_annotation_without_incrementing_symbol_counts_when_symbol_id_is_absent() {
        let annotations = vec![annotation("lib.rs", None, Some((1, 1)))];

        let actual = build_annotation_markers(&annotations);

        assert_eq!(
            AnnotationMarkers {
                symbol_counts: HashMap::new(),
                file_counts: HashMap::from([("lib.rs".to_string(), 1)]),
                line_ranges: HashMap::from([("lib.rs".to_string(), vec![(1, 1)])]),
            },
            actual
        );
    }

    #[test]
    fn should_omit_line_ranges_entry_when_annotation_has_no_range() {
        let annotations = vec![annotation("lib.rs", Some("lib.rs::foo"), None)];

        let actual = build_annotation_markers(&annotations);

        assert_eq!(
            AnnotationMarkers {
                symbol_counts: HashMap::from([("lib.rs::foo".to_string(), 1)]),
                file_counts: HashMap::from([("lib.rs".to_string(), 1)]),
                line_ranges: HashMap::new(),
            },
            actual
        );
    }
}

mod line_has_annotation_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_true_when_line_falls_inside_an_annotation_range() {
        let markers =
            build_annotation_markers(&[annotation("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(true, line_has_annotation(&markers, "lib.rs", 7));
    }

    #[test]
    fn should_return_true_when_line_is_a_range_boundary() {
        let markers =
            build_annotation_markers(&[annotation("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(true, line_has_annotation(&markers, "lib.rs", 5));
        assert_eq!(true, line_has_annotation(&markers, "lib.rs", 10));
    }

    #[test]
    fn should_return_false_when_line_falls_outside_every_annotation_range() {
        let markers =
            build_annotation_markers(&[annotation("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(false, line_has_annotation(&markers, "lib.rs", 11));
    }

    #[test]
    fn should_return_false_when_path_has_no_annotations_at_all() {
        let markers =
            build_annotation_markers(&[annotation("lib.rs", Some("lib.rs::foo"), Some((5, 10)))]);

        assert_eq!(false, line_has_annotation(&markers, "other.rs", 7));
    }
}
