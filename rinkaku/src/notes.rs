//! Diagnostic notes surfaced by `main` — pure helpers extracted from `main.rs`.

/// Applies `--entry <path>` (ADR 0019) to an already-built `Report`: swaps
/// `report.graph` for `graph::pivot_graph`'s re-rooted clone, leaving every
/// other field (`files`, `fan_ins`, `removed`, ...) untouched — the pivot
/// only changes which nodes `render`/`rinkaku-tui` treat as entry points,
/// not what was analyzed.
pub(crate) fn apply_entry_pivot(
    report: rinkaku_core::render::Report,
    path: &str,
) -> rinkaku_core::render::Report {
    let graph = rinkaku_core::graph::pivot_graph(&report.graph, path);
    rinkaku_core::render::Report { graph, ..report }
}

/// Returns a note for `--entry <path>` (ADR 0019) when no symbol's path
/// falls under `path` at all — mirroring `garbage_input_note`/
/// `repo_outline_empty_note`'s existing pure-note-then-`eprintln!`-at-the-
/// call-site pattern rather than having `apply_entry_pivot` itself perform
/// IO. `None` when the report had no symbols to begin with (an empty graph
/// pivoting to an empty graph is not a pivot-specific problem worth a
/// separate note — `garbage_input_note`/`repo_outline_empty_note` already
/// cover that case for their respective input modes).
///
/// Takes the *already-pivoted* `report` (i.e. `apply_entry_pivot`'s own
/// output) rather than re-running `graph::pivot_roots` itself: the call
/// site used to run `pivot_roots` here and then `pivot_graph` (which
/// internally calls `pivot_roots` again) in `apply_entry_pivot`, computing
/// the same root set twice. `graph.roots` on the pivoted report already
/// *is* that root set (`pivot_graph`'s own doc comment), and `graph.nodes`
/// is untouched by pivoting either way, so checking `nodes.is_empty()` for
/// the "no symbols at all" case is equally valid before or after.
pub(crate) fn entry_pivot_empty_note(
    report: &rinkaku_core::render::Report,
    path: &str,
) -> Option<String> {
    if report.graph.nodes.is_empty() {
        return None;
    }
    if report.graph.roots.is_empty() {
        Some(format!("note: no symbols under {path}"))
    } else {
        None
    }
}
/// Returns a warning note for stdin input that is garbage rather than a
/// unified diff — non-empty input that nonetheless produced zero
/// recognized file entries (`parse_unified_diff` never errors on
/// unrecognized text, it simply finds nothing to report, see `diff.rs`),
/// which would otherwise silently exit 0 with an empty report and no
/// indication anything went wrong. `None` when `diff_text` is empty or
/// whitespace-only (already covered by the separate "diff is empty" note
/// at the call site — the two notes are mutually exclusive) or when the
/// report has any file, skip, or test-summary entry at all — a diff that
/// touched only test symbols (ADR 0009's default exclusion moves them out
/// of `files` into `tests`) is a fully-recognized, legitimate result, not
/// garbage input, even though `files`/`skipped` are both empty in that
/// case.
pub(crate) fn garbage_input_note(
    diff_text: &str,
    report: &rinkaku_core::render::Report,
) -> Option<&'static str> {
    if diff_text.trim().is_empty() {
        return None;
    }
    if !report.files.is_empty() || !report.skipped.is_empty() || !report.tests.is_empty() {
        return None;
    }
    Some("note: no file changes recognized in input; expected a unified diff")
}

/// Returns a note for ADR 0017's whole-repo outline when it found nothing
/// to show — every tracked file was either unsupported, a whole test file,
/// generated, or unreadable (`analyze_repo`'s own doc comment: all of these
/// are dropped silently, with no `SkippedFile`/`TestFileSummary` entry to
/// record why, unlike diff mode) — so an empty `stdout` would otherwise
/// look identical to "ran fine, nothing to say" with no indication that a
/// git repository with zero recognizable source files is likely a
/// misconfiguration (wrong directory, `.gitignore`-only repo, etc.).
///
/// Unlike `garbage_input_note`, only `files`/`removed` are checked:
/// `analyze_repo` never populates `skipped`/`tests` at all, so those two
/// fields carry no information in this mode to check against.
pub(crate) fn repo_outline_empty_note(
    report: &rinkaku_core::render::Report,
) -> Option<&'static str> {
    if !report.files.is_empty() || !report.removed.is_empty() {
        return None;
    }
    Some("note: no supported source files found in the repository")
}

#[cfg(test)]
mod tests {
    use super::*;
    mod garbage_input_note_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_core::render::Report;

        fn empty_graph() -> rinkaku_core::graph::SymbolGraph {
            rinkaku_core::graph::SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            }
        }

        fn empty_report() -> Report {
            Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            }
        }

        fn non_empty_report() -> Report {
            Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![rinkaku_core::render::FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![],
                }],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            }
        }

        #[test]
        fn should_return_note_when_input_is_non_empty_but_report_has_no_entries() {
            let actual = garbage_input_note("this is not a diff at all\n", &empty_report());

            assert_eq!(
                Some("note: no file changes recognized in input; expected a unified diff"),
                actual
            );
        }

        #[test]
        fn should_return_none_when_input_is_empty() {
            let actual = garbage_input_note("", &empty_report());

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_input_is_whitespace_only() {
            let actual = garbage_input_note("   \n\n  ", &empty_report());

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_report_has_file_entries() {
            let actual = garbage_input_note("some diff text", &non_empty_report());

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_report_has_only_skipped_entries() {
            let report = Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![rinkaku_core::render::SkippedFile {
                    path: "assets/logo.png".to_string(),
                    reason: rinkaku_core::render::SkipReason::Binary,
                }],
                graph: empty_graph(),
                tests: vec![],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            };

            let actual = garbage_input_note("some diff text", &report);

            assert_eq!(None, actual);
        }

        // Regression test: a diff that touches only test symbols produces a
        // Report with empty files/skipped but a non-empty tests summary
        // (ADR 0009's default exclusion) — a legitimate, fully-recognized
        // diff, not garbage input. Before this fix, garbage_input_note only
        // checked files/skipped, so it wrongly printed "no file changes
        // recognized" for every test-only diff.
        #[test]
        fn should_return_none_when_report_has_only_test_summary_entries() {
            let report = Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![rinkaku_core::render::TestFileSummary {
                    path: "src/lib.rs".to_string(),
                    symbol_count: 1,
                }],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            };

            let actual = garbage_input_note("some diff text", &report);

            assert_eq!(None, actual);
        }

        // Regression test (ADR 0010 follow-up): a diff whose every changed
        // file is `.gitattributes`-generated produces a Report with empty
        // files/tests but a non-empty skipped list of Generated entries —
        // this is still a fully-recognized, legitimate diff, not garbage
        // input, even though the Markdown rendering now hides Generated
        // entries entirely (render.rs's render_markdown) and would
        // therefore render as an empty string. garbage_input_note reads
        // report.skipped directly (never the rendered Markdown string), so
        // this must keep passing without any code change — this test pins
        // that down explicitly rather than leaving it as an implicit
        // consequence of should_return_none_when_report_has_only_skipped_entries
        // above.
        #[test]
        fn should_return_none_when_report_has_only_generated_skip_entries() {
            let report = Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![
                    rinkaku_core::render::SkippedFile {
                        path: "Cargo.lock".to_string(),
                        reason: rinkaku_core::render::SkipReason::Generated,
                    },
                    rinkaku_core::render::SkippedFile {
                        path: "vendor/generated.go".to_string(),
                        reason: rinkaku_core::render::SkipReason::Generated,
                    },
                ],
                graph: empty_graph(),
                tests: vec![],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            };

            let actual = garbage_input_note("some diff text", &report);

            assert_eq!(None, actual);
        }
    }
    mod apply_entry_pivot_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_core::diff::LineRange;
        use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
        use rinkaku_core::render::{FileReport, Report};

        fn symbol(name: &str, referenced_names: Vec<&str>) -> ExtractedSymbol {
            ExtractedSymbol {
                id: String::new(),
                name: name.to_string(),
                kind: SymbolKind::Function,
                signature: format!("fn {name}()"),
                range: LineRange { start: 1, end: 1 },
                container: None,
                referenced_names: referenced_names.into_iter().map(str::to_string).collect(),
                dependencies: vec![],
                omitted_dependency_matches: 0,
                is_test: false,
                classification: None,
                previous_signature: None,
            }
        }

        /// `src/api/handler.rs::api` references `src/util.rs::helper` —
        /// pivoting at "src/api" makes "api" the sole root, mirroring
        /// `graph.rs`'s own pivot-root fixtures. `apply_entry_pivot` is a
        /// thin wrapper over `graph::pivot_graph`, so this module's tests
        /// only pin the wrapper's own contract (every other `Report` field
        /// stays untouched, the note is only printed when appropriate), not
        /// pivot root selection itself.
        fn report_with_api_and_util() -> Report {
            let files = vec![
                FileReport {
                    path: "src/api/handler.rs".to_string(),
                    symbols: vec![symbol("api", vec!["helper"])],
                },
                FileReport {
                    path: "src/util.rs".to_string(),
                    symbols: vec![symbol("helper", vec![])],
                },
            ];
            let graph = rinkaku_core::graph::build_graph(&files);
            Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files,
                skipped: vec![],
                graph,
                tests: vec![],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            }
        }

        #[test]
        fn should_re_root_graph_at_prefix_while_leaving_other_fields_untouched() {
            let report = report_with_api_and_util();

            let actual = apply_entry_pivot(report.clone(), "src/api");

            let expected = Report {
                graph: rinkaku_core::graph::pivot_graph(&report.graph, "src/api"),
                ..report
            };
            assert_eq!(expected, actual);
        }

        #[test]
        fn should_return_no_symbols_under_path_note_when_prefix_matches_nothing() {
            // `entry_pivot_empty_note` now reads `report.graph.roots`
            // directly rather than recomputing `pivot_roots` itself (item 6:
            // avoid pivot-root selection running twice per `--entry`
            // invocation), so its contract requires an already-pivoted
            // report — the same one `apply_entry_pivot` just produced —
            // rather than the raw `build_graph` output `report_with_api_and_util`
            // returns.
            let report = apply_entry_pivot(report_with_api_and_util(), "no/such/path");

            let actual = entry_pivot_empty_note(&report, "no/such/path");

            assert_eq!(
                Some("note: no symbols under no/such/path".to_string()),
                actual
            );
        }

        #[test]
        fn should_return_none_when_prefix_matches_at_least_one_symbol() {
            let report = apply_entry_pivot(report_with_api_and_util(), "src/api");

            let actual = entry_pivot_empty_note(&report, "src/api");

            assert_eq!(None, actual);
        }

        #[test]
        fn should_return_none_when_report_graph_has_no_nodes_at_all() {
            let report = Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: vec![],
                skipped: vec![],
                graph: rinkaku_core::graph::SymbolGraph {
                    nodes: vec![],
                    edges: vec![],
                    roots: vec![],
                },
                tests: vec![],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            };

            let actual = entry_pivot_empty_note(&report, "src/api");

            assert_eq!(None, actual);
        }
    }
    mod repo_outline_empty_note_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_core::render::Report;

        fn empty_graph() -> rinkaku_core::graph::SymbolGraph {
            rinkaku_core::graph::SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            }
        }

        fn empty_report() -> Report {
            Report {
                origin: rinkaku_core::render::ReportOrigin::RepoOutline,
                files: vec![],
                skipped: vec![],
                graph: empty_graph(),
                tests: vec![],
                fan_ins: vec![],
                file_size_warnings: vec![],
                file_size_bands: vec![],
                removed: vec![],
            }
        }

        #[test]
        fn should_return_note_when_report_has_no_files_and_no_removed() {
            let actual = repo_outline_empty_note(&empty_report());

            assert_eq!(
                Some("note: no supported source files found in the repository"),
                actual
            );
        }

        #[test]
        fn should_return_none_when_report_has_file_entries() {
            let report = Report {
                files: vec![rinkaku_core::render::FileReport {
                    path: "src/lib.rs".to_string(),
                    symbols: vec![],
                }],
                ..empty_report()
            };

            let actual = repo_outline_empty_note(&report);

            assert_eq!(None, actual);
        }

        // Regression test: `analyze_repo` leaves `removed` empty on every
        // path today (ADR 0017's whole point is that nothing changed, so
        // there is no base side to diff against), but the check still
        // covers it explicitly so a future extension to `analyze_repo`
        // doesn't silently regress this note into firing on a report that
        // does have something to show.
        #[test]
        fn should_return_none_when_report_has_removed_entries() {
            let report = Report {
                removed: vec![rinkaku_core::extract::RemovedSymbol {
                    name: "old_helper".to_string(),
                    kind: rinkaku_core::extract::SymbolKind::Function,
                    path: "src/lib.rs".to_string(),
                    signature: "fn old_helper()".to_string(),
                }],
                ..empty_report()
            };

            let actual = repo_outline_empty_note(&report);

            assert_eq!(None, actual);
        }
    }
}
