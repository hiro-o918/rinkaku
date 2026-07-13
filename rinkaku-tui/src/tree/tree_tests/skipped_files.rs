use super::*;
use pretty_assertions::assert_eq;

// Skipped-file tests: a file rinkaku could not extract symbols from
// (unsupported language, binary, deleted) must still show up in the
// tree, since otherwise it is invisible to a reviewer relying on the
// TUI to see the whole PR (the user-reported gap this feature closes).

#[test]
fn should_add_skipped_file_as_childless_file_node_with_skip_reason() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        skipped: vec![SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: rinkaku_core::render::SkipReason::Binary,
        }],
        ..empty_report()
    };

    let expected = Tree {
        roots: vec![TreeNode {
            kind: NodeKind::Dir,
            path: "assets".to_string(),
            badges: Badges::default(),
            children: vec![TreeNode {
                kind: NodeKind::File,
                path: "assets/logo.png".to_string(),
                badges: Badges::default(),
                children: vec![],
                skip_reason: Some(rinkaku_core::render::SkipReason::Binary),
                test_symbol_count: None,
            }],
            skip_reason: None,
            test_symbol_count: None,
        }],
    };
    let actual = build_tree(&report);

    assert_eq!(expected, actual);
}

#[test]
fn should_omit_generated_skip_reason_from_tree_by_default() {
    // Mirrors `render_markdown`'s own `SkipReason::Generated` filter
    // (ADR 0010/0011): a `.gitattributes`-declared or content-marked
    // generated file is already known-uninteresting, so it should not
    // clutter the TUI tree either.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        skipped: vec![SkippedFile {
            path: "Cargo.lock".to_string(),
            reason: rinkaku_core::render::SkipReason::Generated,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(Tree { roots: vec![] }, tree);
}

#[test]
fn should_keep_non_generated_skip_reasons_when_mixed_with_a_generated_entry() {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        skipped: vec![
            SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: rinkaku_core::render::SkipReason::Generated,
            },
            SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            },
        ],
        ..empty_report()
    };

    let tree = build_tree(&report);

    let paths: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["assets"], paths);
}

#[test]
fn should_merge_skipped_file_into_existing_dir_alongside_analyzed_files() {
    // A skipped file sharing a directory with an already-analyzed file
    // must land in the same `Dir` node, not create a second "src" root.
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/lib.rs".to_string(),
            symbols: vec![],
        }],
        skipped: vec![SkippedFile {
            path: "src/generated.pb.go".to_string(),
            reason: rinkaku_core::render::SkipReason::UnsupportedLanguage,
        }],
        ..empty_report()
    };

    let tree = build_tree(&report);

    assert_eq!(1, tree.roots.len());
    let src = &tree.roots[0];
    assert_eq!("src", src.path);
    assert_eq!(2, src.children.len());
    // Source order: `report.files` is inserted before `report.skipped`
    // (see `build_tree`'s own doc comment), so the analyzed file comes
    // first even though "generated.pb.go" sorts first alphabetically.
    let paths: Vec<&str> = src.children.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(vec!["src/lib.rs", "src/generated.pb.go"], paths);
}
