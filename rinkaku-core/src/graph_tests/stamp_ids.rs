use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_stamp_each_symbol_id_when_graph_has_no_duplicate_names() {
    let mut files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![symbol("foo", vec!["bar"]), symbol("bar", vec![])],
    }];
    let graph = build_graph(&files);

    stamp_ids(&mut files, &graph);

    let expected = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            ExtractedSymbol {
                id: "src/lib.rs::foo".to_string(),
                ..symbol("foo", vec!["bar"])
            },
            ExtractedSymbol {
                id: "src/lib.rs::bar".to_string(),
                ..symbol("bar", vec![])
            },
        ],
    }];

    assert_eq!(expected, files);
}

#[test]
fn should_stamp_disambiguated_id_when_duplicate_path_and_name() {
    let mut files = vec![FileReport {
        path: "src/lib.rs".to_string(),
        symbols: vec![
            ExtractedSymbol {
                range: LineRange { start: 1, end: 2 },
                ..symbol("foo", vec![])
            },
            ExtractedSymbol {
                range: LineRange { start: 10, end: 12 },
                ..symbol("foo", vec![])
            },
        ],
    }];
    let graph = build_graph(&files);

    stamp_ids(&mut files, &graph);

    let expected_ids = vec![
        "src/lib.rs::foo@1".to_string(),
        "src/lib.rs::foo@10".to_string(),
    ];
    let actual_ids: Vec<String> = files[0].symbols.iter().map(|s| s.id.clone()).collect();

    assert_eq!(expected_ids, actual_ids);
}
