use super::*;
use crate::app::App;
use crate::diff_view::{FileHunks, Hunk};
use crate::{derive_selection_snapshot, dispatch_note_compose_key, first_anchor_run};

fn hunk(new_range: Option<(usize, usize)>) -> Hunk {
    Hunk {
        header: "@@ @@".to_string(),
        new_range,
        lines: vec![],
    }
}

mod first_anchor_run_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_return_the_clamped_overlap_of_the_first_intersecting_hunk() {
        let file_hunks = FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(Some((5, 10)))],
        };

        let actual = first_anchor_run(&file_hunks, (1, 8));

        assert_eq!(Some((5, 8)), actual);
    }

    #[test]
    fn should_skip_non_intersecting_hunks_and_use_the_first_that_intersects() {
        let file_hunks = FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(Some((100, 110))), hunk(Some((5, 10)))],
        };

        let actual = first_anchor_run(&file_hunks, (1, 8));

        assert_eq!(Some((5, 8)), actual);
    }

    #[test]
    fn should_return_none_when_no_hunk_intersects_the_range() {
        let file_hunks = FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(Some((100, 110)))],
        };

        let actual = first_anchor_run(&file_hunks, (1, 8));

        assert_eq!(None, actual);
    }

    #[test]
    fn should_skip_a_pure_deletion_hunk_with_a_zero_width_new_range() {
        let file_hunks = FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(Some((5, 4))), hunk(Some((5, 10)))],
        };

        let actual = first_anchor_run(&file_hunks, (1, 8));

        assert_eq!(Some((5, 8)), actual);
    }

    #[test]
    fn should_return_none_when_there_are_no_hunks() {
        let file_hunks = FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![],
        };

        let actual = first_anchor_run(&file_hunks, (1, 8));

        assert_eq!(None, actual);
    }
}

mod derive_selection_snapshot_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_build_a_snapshot_with_anchor_when_cursor_is_on_a_symbol_row_with_an_intersecting_hunk()
     {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(Some((1, 1)))],
        }];

        let actual = derive_selection_snapshot(&app, &report, &diff_files);

        assert_eq!(
            Some(crate::review::SelectionSnapshot {
                path: "lib.rs".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((1, 1)),
                anchor: Some((1, 1)),
                signature: Some("fn foo()".to_string()),
            }),
            actual
        );
    }

    #[test]
    fn should_build_a_snapshot_with_no_anchor_when_no_hunk_intersects_the_symbol() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(Some((100, 110)))],
        }];

        let actual = derive_selection_snapshot(&app, &report, &diff_files);

        assert_eq!(
            Some(crate::review::SelectionSnapshot {
                path: "lib.rs".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((1, 1)),
                anchor: None,
                signature: Some("fn foo()".to_string()),
            }),
            actual
        );
    }

    #[test]
    fn should_return_none_when_cursor_is_on_a_directory_row() {
        let report = Report {
            files: vec![
                rinkaku_core::render::FileReport {
                    path: "a/one.rs".to_string(),
                    symbols: vec![],
                },
                rinkaku_core::render::FileReport {
                    path: "b/two.rs".to_string(),
                    symbols: vec![],
                },
            ],
            ..empty_report()
        };
        let app = App::new(&report);

        let actual = derive_selection_snapshot(&app, &report, &[]);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_none_when_no_diff_hunks_cover_the_symbols_file() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);

        let actual = derive_selection_snapshot(&app, &report, &[]);

        assert_eq!(
            Some(crate::review::SelectionSnapshot {
                path: "lib.rs".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                symbol_name: Some("foo".to_string()),
                range: Some((1, 1)),
                anchor: None,
                signature: Some("fn foo()".to_string()),
            }),
            actual
        );
    }
}

mod dispatch_note_compose_key_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_open_compose_overlay_when_snapshot_is_some() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::Down);
        let snapshot = derive_selection_snapshot(&app, &report, &[]);
        assert!(snapshot.is_some());

        let actual = dispatch_note_compose_key(app, snapshot);

        assert!(matches!(
            actual.review().mode(),
            crate::review::ReviewMode::Compose { .. }
        ));
    }

    #[test]
    fn should_clear_pending_prefix_when_snapshot_is_none() {
        // Regression test (ADR 0022's `pending_prefix` bug, same class):
        // pressing `n` over a row with no derivable snapshot (a directory
        // row, or the source screen) must still discard a pending `g`
        // prefix — otherwise a `g` press followed by an ineffective `n`
        // leaves `pending_prefix` stuck at `Some(G)`, and the *next* `d`
        // the reviewer types for its own ordinary reason (`ToggleDiff`)
        // silently resolves as `GotoDefinition` instead.
        let report = empty_report();
        let app = App::new(&report).handle_key(crate::app::InputKey::PendingGoto);
        assert_eq!(Some(crate::app::PendingPrefix::G), app.pending_prefix());

        let actual = dispatch_note_compose_key(app, None);

        assert_eq!(None, actual.pending_prefix());
    }

    #[test]
    fn should_leave_review_idle_when_snapshot_is_none() {
        let report = empty_report();
        let app = App::new(&report);

        let actual = dispatch_note_compose_key(app, None);

        assert_eq!(&crate::review::ReviewMode::Idle, actual.review().mode());
    }
}
