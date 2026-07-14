//! Source drill-down (ADR 0015): given a selected symbol, reads its file
//! from disk and computes which lines to show, scrolled to and
//! highlighting the symbol's own range.
//!
//! ADR 0016 explicitly allows adapter-side file reads in `rinkaku-tui` for
//! this feature ("All IO the TUI needs beyond `Report` itself... is
//! adapter-side file reads in `rinkaku-tui`"), on the condition that it
//! stays isolated to one small function rather than spreading IO through
//! the rest of the crate. [`load_symbol_source`] is that one function;
//! everything else here (locating the symbol, computing the visible
//! window) is pure and takes the file content as a plain `&str`.
//!
//! `Report` paths are always repository-root-relative (every `main.rs`
//! input mode — stdin, `--base`, `--pr` — derives them from `git diff`/
//! `git ls-files` output, never from the process's current directory), so
//! reading them off disk needs the repository root joined in first rather
//! than treating them as relative to wherever `rinkaku` happens to be
//! invoked from (e.g. a subdirectory) — see [`resolve_source_path`].
//!
//! **File content itself comes from a [`SourceReader`] (ADR 0047), not a
//! hardcoded `std::fs::read_to_string` call.** `rinkaku-tui` never shells
//! out to `git` (ADR 0016), so `--pr` mode's IO — reading a file as it
//! existed at the PR's head commit, via `git show <sha>:<path>` — is
//! implemented in `rinkaku`'s own adapter layer and injected in through
//! this trait; [`WorkingTreeSourceReader`] is the default every other
//! input mode uses.

/// A symbol's location in `Report`, resolved from its id — enough for
/// [`load_symbol_source`] to know which file to read and
/// [`visible_window`] to know which lines to highlight.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolLocation {
    path: String,
    /// 1-based inclusive, matching `ExtractedSymbol::range`'s own
    /// convention (see that field's doc comment in `rinkaku-core`).
    start_line: usize,
    end_line: usize,
}

/// The result of opening a symbol's source: its full file content, split
/// into lines, plus the 1-based inclusive line range to highlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceView {
    pub path: String,
    pub lines: Vec<String>,
    pub highlight_start: usize,
    pub highlight_end: usize,
}

/// A [`SourceView`] plus its per-line syntax highlighting (ADR 0018's
/// tree-sitter-highlight stack, extended to the source screen) — computed
/// together, once, by [`load_highlighted_symbol_source`] so `crate::ui`'s
/// source screen never needs to touch disk or re-parse on every frame (see
/// that function's own doc comment; mirrors `crate::highlight::HighlightedFile`
/// pairing a file's hunks with their highlight data the same way).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightedSourceView {
    pub view: SourceView,
    /// One entry per `view.lines`, same length/order — `crate::ui`'s
    /// `source_lines` reads `token_highlights[i]` for `view.lines[i]`.
    pub token_highlights: Vec<crate::highlight::LineHighlight>,
}

/// A source of file content for the source drill-down (ADR 0047),
/// injected so this crate's only IO stays behind a small port rather than
/// a hardcoded `std::fs` call — `rinkaku-tui` never shells out to `git`
/// itself (ADR 0016), so a reader backed by `git show` lives in `rinkaku`'s
/// adapter layer and is wired in from there.
pub trait SourceReader {
    /// Reads `relative_path` (repository-root-relative, see this module's
    /// doc comment) and returns its content, or an error message suitable
    /// for the status line on failure.
    fn read(&self, repo_root: &std::path::Path, relative_path: &str) -> Result<String, String>;
}

/// The default [`SourceReader`]: reads `relative_path` off the working
/// tree, joined onto `repo_root` via [`resolve_source_path`]. Used by
/// every input mode except `--pr` (ADR 0047), for which `main.rs` wires in
/// a `git show <head SHA>:<path>`-backed reader instead so the source view
/// reflects the PR's actual head snapshot rather than whatever happens to
/// be checked out locally.
pub struct WorkingTreeSourceReader;

impl SourceReader for WorkingTreeSourceReader {
    fn read(&self, repo_root: &std::path::Path, relative_path: &str) -> Result<String, String> {
        let full_path = resolve_source_path(repo_root, relative_path);
        std::fs::read_to_string(&full_path).map_err(|source| {
            format!(
                "failed to read {}: {source} (not present in the working tree — expected for a \
                 file diffed from a PR or historical commit not checked out locally)",
                full_path.display()
            )
        })
    }
}

/// Reads `id`'s file via `reader` and builds a [`SourceView`] for it, or an
/// error message suitable for the status line on failure (no such symbol
/// in `report`, or the file read itself failing).
///
/// `repo_root` anchors `location.path` (always repository-root-relative,
/// see this module's doc comment) — callers pass the repository root
/// `main.rs` resolves once at startup (`rinkaku_tui::run`'s own
/// `repo_root` parameter), not the process's current directory, so this
/// still works when `rinkaku` is invoked from a subdirectory of the
/// repository.
///
/// With [`WorkingTreeSourceReader`] (every input mode except `--pr`), a
/// symbol's `range` in `report` reflects the file's content *at analysis
/// time*. If the file is edited on disk afterward (including between
/// opening the TUI and pressing `s` on a given row), the highlighted lines
/// can drift from the symbol's actual current location, or — if the file
/// shrank — extend past its current end entirely. [`visible_window`]
/// clamps to the file's current length either way rather than producing
/// an out-of-bounds window, but it makes no attempt to re-locate the
/// symbol in the changed content.
pub fn load_symbol_source(
    report: &rinkaku_core::render::Report,
    id: &str,
    repo_root: &std::path::Path,
    reader: &dyn SourceReader,
) -> Result<SourceView, String> {
    let location = find_symbol_location(report, id)
        .ok_or_else(|| format!("symbol not found in report: {id}"))?;

    let content = reader.read(repo_root, &location.path)?;

    Ok(SourceView {
        path: location.path,
        lines: content.lines().map(str::to_string).collect(),
        highlight_start: location.start_line,
        highlight_end: location.end_line,
    })
}

/// [`load_symbol_source`] plus syntax highlighting
/// (`crate::highlight::highlight_source_lines`), composed together as the
/// single IO+highlight step `crate::run_app` performs exactly once when the
/// `s` key opens [`crate::app::Screen::Source`] — not inside the render
/// loop. Highlighting a file is a full tree-sitter parse, strictly more
/// expensive than the plain file read `load_symbol_source` alone requires
/// (mirroring ADR 0018's own "highlighting must not run inside the render
/// loop" rule for the diff pane, `crate::highlight::highlight_diff_files`'s
/// own doc comment), so this must be called once per `s` press and its
/// result cached by the caller, the same discipline `crate::run_app`
/// already applies to `diff_pane_content`/`blast_radius_selection`.
///
/// Returns the same `Err` as `load_symbol_source` on failure (unchanged
/// error surface for `crate::run_app`'s status-line reporting) — a file
/// that fails to load never reaches the highlighting step.
pub fn load_highlighted_symbol_source(
    report: &rinkaku_core::render::Report,
    id: &str,
    repo_root: &std::path::Path,
    reader: &dyn SourceReader,
) -> Result<HighlightedSourceView, String> {
    let view = load_symbol_source(report, id, repo_root, reader)?;
    let token_highlights = crate::highlight::highlight_source_lines(&view.path, &view.lines);
    Ok(HighlightedSourceView {
        view,
        token_highlights,
    })
}

/// Joins a `Report`-relative path (always repository-root-relative, see
/// this module's doc comment) onto `repo_root`, so [`load_symbol_source`]
/// reads the right file regardless of the process's current directory.
/// Split out as its own pure function so the join logic is unit-testable
/// without touching disk.
///
/// Relies on `relative_path` genuinely being relative: `PathBuf::join`
/// silently *discards* `repo_root` entirely and returns `relative_path`
/// unchanged whenever it is itself absolute (`Path::join`'s documented
/// behavior) — every producer of `Report` (`git diff`/`git ls-files`
/// output, see this module's doc comment) upholds that today, so this
/// isn't reachable in practice, but the `debug_assert!` below turns a
/// future violation of that premise into a loud failure in tests/debug
/// builds instead of a silent wrong-file read.
fn resolve_source_path(repo_root: &std::path::Path, relative_path: &str) -> std::path::PathBuf {
    debug_assert!(
        std::path::Path::new(relative_path).is_relative(),
        "Report paths are always repository-root-relative, got absolute path: {relative_path}"
    );
    repo_root.join(relative_path)
}

/// Finds `id`'s file path and line range in `report.files`. `None` when no
/// symbol with that id is present — e.g. `id` refers to a removed symbol
/// (out of scope, same as `crate::detail::build_detail`'s contract: a
/// `RemovedSymbol` has no stable id or line range to highlight, only the
/// prior signature `Report.removed` already carries).
fn find_symbol_location(report: &rinkaku_core::render::Report, id: &str) -> Option<SymbolLocation> {
    report.files.iter().find_map(|file| {
        file.symbols
            .iter()
            .find(|symbol| symbol.id == id)
            .map(|symbol| SymbolLocation {
                path: file.path.clone(),
                start_line: symbol.range.start,
                end_line: symbol.range.end,
            })
    })
}

/// Computes the 0-based `[start, end)` slice of `total_lines` to display
/// in a viewport `viewport_height` rows tall, centering `highlight_start`/
/// `highlight_end` (1-based inclusive, matching `SourceView`'s own fields)
/// when the symbol's range is smaller than the viewport, and clamping to
/// `[0, total_lines)` so a symbol near the top/bottom of a short file never
/// asks for an out-of-bounds slice.
///
/// Centering (rather than e.g. always putting the highlight at the top)
/// keeps a few lines of leading context visible for a mid-function
/// selection, matching how an editor's "jump to definition" typically
/// scrolls.
pub fn visible_window(
    total_lines: usize,
    highlight_start: usize,
    highlight_end: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if total_lines == 0 || viewport_height == 0 {
        return (0, 0);
    }

    // Convert to 0-based for arithmetic; a `highlight_start` of 0 (out of
    // range for the 1-based convention, defensive only) is clamped to 1
    // first so the subtraction below cannot underflow.
    let highlight_start = highlight_start.max(1) - 1;
    let highlight_end = highlight_end.max(1) - 1;
    let highlight_center = highlight_start + (highlight_end.saturating_sub(highlight_start)) / 2;

    let half = viewport_height / 2;
    let ideal_start = highlight_center.saturating_sub(half);

    // Clamp so the window never runs past the end of the file, then
    // clamp again at zero so a short file (fewer lines than
    // `viewport_height`) still yields a valid, in-bounds window rather
    // than a negative start.
    let max_start = total_lines.saturating_sub(viewport_height);
    let start = ideal_start.min(max_start);
    let end = (start + viewport_height).min(total_lines);

    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_center_window_around_highlight_when_file_is_larger_than_viewport() {
        let (start, end) = visible_window(100, 50, 50, 10);

        // Highlight at 0-based line 49, viewport 10 rows -> half=5,
        // ideal_start = 49 - 5 = 44.
        assert_eq!((44, 54), (start, end));
    }

    #[test]
    fn should_clamp_window_to_start_of_file_when_highlight_is_near_the_top() {
        let (start, end) = visible_window(100, 1, 1, 10);

        assert_eq!((0, 10), (start, end));
    }

    #[test]
    fn should_clamp_window_to_end_of_file_when_highlight_is_near_the_bottom() {
        let (start, end) = visible_window(100, 100, 100, 10);

        assert_eq!((90, 100), (start, end));
    }

    #[test]
    fn should_show_whole_file_when_file_is_shorter_than_viewport() {
        let (start, end) = visible_window(5, 3, 3, 10);

        assert_eq!((0, 5), (start, end));
    }

    #[test]
    fn should_return_empty_window_when_file_has_no_lines() {
        let (start, end) = visible_window(0, 1, 1, 10);

        assert_eq!((0, 0), (start, end));
    }

    #[test]
    fn should_return_empty_window_when_viewport_height_is_zero() {
        let (start, end) = visible_window(100, 1, 1, 0);

        assert_eq!((0, 0), (start, end));
    }

    #[test]
    fn should_span_multi_line_highlight_range_at_its_midpoint() {
        // Highlight spans lines 10-20 (1-based inclusive); midpoint
        // (0-based) is 9 + (19-9)/2 = 14.
        let (start, _end) = visible_window(1000, 10, 20, 10);

        assert_eq!(9, start);
    }

    #[test]
    fn should_clamp_to_end_of_file_when_highlight_range_exceeds_current_line_count() {
        // The symbol's range (50-60) came from analysis time; the file has
        // since shrunk to 10 lines (e.g. edited in the working tree after
        // the diff was analyzed — `load_symbol_source`'s own doc comment
        // notes the source view always reads the *current* working tree,
        // not the analyzed commit). Neither endpoint of the highlight is a
        // valid line any more, so this must degrade to "clamp to the end
        // of the file" rather than producing an out-of-bounds or empty
        // window.
        let (start, end) = visible_window(10, 50, 60, 10);

        assert_eq!((0, 10), (start, end));
    }

    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;
    use rinkaku_core::render::{FileReport, Report};

    fn symbol(id: &str, name: &str, range: LineRange) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range,
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    fn empty_report() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            file_size_bands: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_find_symbol_location_from_matching_file() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    LineRange { start: 10, end: 20 },
                )],
            }],
            ..empty_report()
        };

        let expected = Some(SymbolLocation {
            path: "src/lib.rs".to_string(),
            start_line: 10,
            end_line: 20,
        });
        let actual = find_symbol_location(&report, "src/lib.rs::foo");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_none_when_symbol_id_is_not_found() {
        let report = empty_report();

        let actual = find_symbol_location(&report, "missing::id");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_error_message_when_load_symbol_source_finds_no_such_symbol() {
        let report = empty_report();

        let actual = load_symbol_source(
            &report,
            "missing::id",
            std::path::Path::new("/repo"),
            &WorkingTreeSourceReader,
        );

        assert_eq!(
            Err("symbol not found in report: missing::id".to_string()),
            actual
        );
    }

    #[test]
    fn should_join_repo_root_and_relative_path_when_resolving_source_path() {
        let actual = resolve_source_path(std::path::Path::new("/repo/root"), "src/lib.rs");

        assert_eq!(std::path::PathBuf::from("/repo/root/src/lib.rs"), actual);
    }

    #[test]
    #[should_panic(expected = "Report paths are always repository-root-relative")]
    fn should_panic_in_debug_builds_when_relative_path_is_actually_absolute() {
        // Pins the `debug_assert!`'s intent: `PathBuf::join` silently
        // *discards* `repo_root` and returns the absolute path unchanged
        // when the "relative" argument isn't actually relative
        // (`resolve_source_path`'s own doc comment) — every `Report`
        // producer upholds relativity today, so this only guards against a
        // future regression, but that regression must fail loudly in
        // debug/test builds rather than silently reading the wrong file.
        resolve_source_path(std::path::Path::new("/repo/root"), "/etc/passwd");
    }

    #[test]
    fn should_read_file_relative_to_repo_root_when_process_cwd_differs() {
        // Regression test for the TUI source view failing whenever
        // `rinkaku` is launched from a subdirectory of the repository:
        // `Report` paths are always repo-root-relative, so
        // `load_symbol_source` must join them onto the repo root rather
        // than reading them relative to the process's actual current
        // directory (which this test never changes).
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("src")).expect("create src dir");
        std::fs::write(dir.path().join("src/lib.rs"), "fn foo() {}\n").expect("write file");

        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    LineRange { start: 1, end: 1 },
                )],
            }],
            ..empty_report()
        };

        let expected = Ok(SourceView {
            path: "src/lib.rs".to_string(),
            lines: vec!["fn foo() {}".to_string()],
            highlight_start: 1,
            highlight_end: 1,
        });
        let actual = load_symbol_source(
            &report,
            "src/lib.rs::foo",
            dir.path(),
            &WorkingTreeSourceReader,
        );

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_mention_working_tree_in_error_message_when_file_is_missing() {
        let dir = tempfile::tempdir().expect("create temp dir");

        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/missing.rs".to_string(),
                symbols: vec![symbol(
                    "src/missing.rs::foo",
                    "foo",
                    LineRange { start: 1, end: 1 },
                )],
            }],
            ..empty_report()
        };

        let actual = load_symbol_source(
            &report,
            "src/missing.rs::foo",
            dir.path(),
            &WorkingTreeSourceReader,
        );

        let error = actual.expect_err("a missing file must fail rather than silently succeed");
        assert!(
            error.contains("not present in the working tree"),
            "error message should explain the file may not be checked out locally, got: {error}"
        );
    }

    /// A [`SourceReader`] that always returns fixed content, ignoring both
    /// arguments — used to prove [`load_symbol_source`] actually reads
    /// through the injected reader rather than reaching for the working
    /// tree directly.
    struct FakeSourceReader {
        content: Result<String, String>,
    }

    impl SourceReader for FakeSourceReader {
        fn read(
            &self,
            _repo_root: &std::path::Path,
            _relative_path: &str,
        ) -> Result<String, String> {
            self.content.clone()
        }
    }

    #[test]
    fn should_read_via_injected_reader_when_loading_symbol_source() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    LineRange { start: 1, end: 1 },
                )],
            }],
            ..empty_report()
        };
        let reader = FakeSourceReader {
            content: Ok("fn foo() { /* head snapshot */ }".to_string()),
        };

        let actual = load_symbol_source(
            &report,
            "src/lib.rs::foo",
            std::path::Path::new("/unused"),
            &reader,
        );

        assert_eq!(
            Ok(SourceView {
                path: "src/lib.rs".to_string(),
                lines: vec!["fn foo() { /* head snapshot */ }".to_string()],
                highlight_start: 1,
                highlight_end: 1,
            }),
            actual
        );
    }

    #[test]
    fn should_propagate_reader_error_when_loading_symbol_source() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    LineRange { start: 1, end: 1 },
                )],
            }],
            ..empty_report()
        };
        let reader = FakeSourceReader {
            content: Err("git show origin/pr-head:src/lib.rs failed".to_string()),
        };

        let actual = load_symbol_source(
            &report,
            "src/lib.rs::foo",
            std::path::Path::new("/unused"),
            &reader,
        );

        assert_eq!(
            Err("git show origin/pr-head:src/lib.rs failed".to_string()),
            actual
        );
    }

    // --- load_highlighted_symbol_source ---

    #[test]
    fn should_load_and_highlight_source_together_for_a_recognized_extension() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir_all(dir.path().join("src")).expect("create src dir");
        std::fs::write(dir.path().join("src/lib.rs"), "fn foo() {}\n").expect("write file");

        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "src/lib.rs::foo",
                    "foo",
                    LineRange { start: 1, end: 1 },
                )],
            }],
            ..empty_report()
        };

        let actual = load_highlighted_symbol_source(
            &report,
            "src/lib.rs::foo",
            dir.path(),
            &WorkingTreeSourceReader,
        )
        .expect("expected a successful load for an existing .rs file");

        assert_eq!(
            SourceView {
                path: "src/lib.rs".to_string(),
                lines: vec!["fn foo() {}".to_string()],
                highlight_start: 1,
                highlight_end: 1,
            },
            actual.view
        );
        assert_eq!(1, actual.token_highlights.len());
        let spans = actual.token_highlights[0]
            .clone()
            .expect("expected Some(spans) for a .rs file");
        let keyword_index = crate::highlight::PALETTE
            .iter()
            .position(|p| *p == "keyword")
            .unwrap();
        assert!(
            spans
                .iter()
                .any(|s| s.start == 0 && s.end == 2 && s.palette_index == keyword_index)
        );
    }

    #[test]
    fn should_fall_back_to_none_highlights_for_an_unrecognized_extension() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("config.yaml"), "key: value\n").expect("write file");

        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "config.yaml".to_string(),
                symbols: vec![symbol(
                    "config.yaml::root",
                    "root",
                    LineRange { start: 1, end: 1 },
                )],
            }],
            ..empty_report()
        };

        let actual = load_highlighted_symbol_source(
            &report,
            "config.yaml::root",
            dir.path(),
            &WorkingTreeSourceReader,
        )
        .expect("expected a successful load for an existing file");

        assert_eq!(vec![None], actual.token_highlights);
    }

    #[test]
    fn should_propagate_load_error_without_attempting_to_highlight() {
        let dir = tempfile::tempdir().expect("create temp dir");

        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/missing.rs".to_string(),
                symbols: vec![symbol(
                    "src/missing.rs::foo",
                    "foo",
                    LineRange { start: 1, end: 1 },
                )],
            }],
            ..empty_report()
        };

        let actual = load_highlighted_symbol_source(
            &report,
            "src/missing.rs::foo",
            dir.path(),
            &WorkingTreeSourceReader,
        );

        let error = actual.expect_err("a missing file must fail rather than silently succeed");
        assert!(
            error.contains("not present in the working tree"),
            "error message should explain the file may not be checked out locally, got: {error}"
        );
    }
}
