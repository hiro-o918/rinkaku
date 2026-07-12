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

/// Reads `id`'s file off the working tree and builds a [`SourceView`] for
/// it, or an error message suitable for the status line on failure (no
/// such symbol in `report`, or the file read itself failing — a moved/
/// deleted file since the diff was analyzed, a permissions error, etc.).
///
/// Always reads from the working tree, regardless of which input mode
/// (`--base`, `--pr`, stdin) produced `report` — unlike `main.rs`'s
/// `--base`/`--pr` pipelines, which read historical content via
/// `git show <rev>:<path>` to stay pinned to the exact diffed commit, the
/// TUI's source view is a live "look at the file now" drill-down, so
/// reading the working tree is the right behavior even when `report` was
/// built from a historical diff (the alternative — plumbing the resolved
/// head SHA all the way from `main.rs` into `rinkaku_tui::run` just for
/// this one view — is deferred until a real user need for it shows up).
///
/// A consequence of reading live: a symbol's `range` in `report` reflects
/// the file's content *at analysis time*. If the file is edited on disk
/// afterward (including between opening the TUI and pressing `s` on a
/// given row), the highlighted lines can drift from the symbol's actual
/// current location, or — if the file shrank — extend past its current
/// end entirely. [`visible_window`] clamps to the file's current length
/// either way rather than producing an out-of-bounds window, but it makes
/// no attempt to re-locate the symbol in the changed content.
pub fn load_symbol_source(
    report: &rinkaku_core::render::Report,
    id: &str,
) -> Result<SourceView, String> {
    let location = find_symbol_location(report, id)
        .ok_or_else(|| format!("symbol not found in report: {id}"))?;

    let content = std::fs::read_to_string(&location.path)
        .map_err(|source| format!("failed to read {}: {source}", location.path))?;

    Ok(SourceView {
        path: location.path,
        lines: content.lines().map(str::to_string).collect(),
        highlight_start: location.start_line,
        highlight_end: location.end_line,
    })
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
            files: vec![],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            hotspots: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_find_symbol_location_from_matching_file() {
        let report = Report {
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

        let actual = load_symbol_source(&report, "missing::id");

        assert_eq!(
            Err("symbol not found in report: missing::id".to_string()),
            actual
        );
    }
}
