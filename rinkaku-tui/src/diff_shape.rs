//! Diff-pane content shaping (ADR 0020, ADR 0027, ADR 0030, ADR 0072): given
//! the row currently selected in the entry view (a symbol or a file) plus
//! the already-parsed diff hunks (`crate::diff_view`), decides what the diff
//! pane shows. Per ADR 0072 both symbol-row and file-row selections show the
//! same content — the whole file's hunks, in original `git diff` order, with
//! no per-symbol grouping; a symbol selection only changes where the pane
//! auto-scrolls to ([`section_start_line_for_symbol`], despite the name kept
//! for continuity with ADR 0027/0030's naming, now resolves the selected
//! symbol's *first intersecting hunk*, not a section start). ADR 0030 adds
//! the mirror image — [`symbol_id_for_scroll_line`] resolves a scroll offset
//! back to the symbol whose line range the hunk at that offset intersects,
//! so `crate::run_app` can sync the tree cursor when the reviewer scrolls
//! the pane manually.
//!
//! Pure and free of `ratatui` types, mirroring every other view-model in
//! this crate (`crate::tree`/`crate::nav`/`crate::detail`/`crate::blast_radius`):
//! `Report` + `&[FileHunks]` + a selection in, plain [`DiffPaneContent`]
//! data out. `crate::run_app` computes this once per handled key (the same
//! cache-on-selection-change discipline `crate::app::App::selected_blast_radius_view`'s
//! own doc comment already establishes, after that pane's own past
//! per-frame recompute bug — see this crate's `lib.rs` regression test);
//! `crate::ui::draw` must not call it, for the identical reason
//! `ui::draw` must not call `App::selected_blast_radius_view` either.

use crate::app::DiffTarget;
use crate::diff_view::{FileHunks, Hunk, file_hunks, hunk_intersects};
pub use crate::split_pairing::{SplitRow, pair_hunk_lines};
use rinkaku_core::diff::LineRange;
use rinkaku_core::render::Report;

/// One [`Hunk`], cloned out of the original [`FileHunks`] into
/// [`DiffPaneContent::File`] (this module's own doc comment on why
/// cloning, not borrowing), plus its `source_index` — its position in that
/// original `FileHunks::hunks` slice. `crate::highlight::lookup_hunk_highlight_by_index`
/// looks up a hunk's precomputed highlight by this index rather than
/// pointer identity, since a clone breaks the pointer identity
/// `crate::highlight::lookup_hunk_highlight` otherwise relies on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributedHunk {
    pub source_index: usize,
    pub hunk: Hunk,
}

/// The diff pane's fully shaped content for the current selection —
/// what `crate::ui::draw_diff_pane` renders, computed once by
/// `crate::run_app` and handed in rather than recomputed per draw.
///
/// Per ADR 0072 there is exactly one non-empty shape: the whole file's
/// hunks in original order. Symbol vs. file selection differ only in
/// initial scroll target ([`section_start_line_for_symbol`]), not in what
/// content is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffPaneContent {
    /// Nothing to show: no row selected, a directory row (no diff-specific
    /// content of its own — `App::selected_diff_target`'s own doc comment),
    /// or (defensively) a mismatch between `report` and the diff text.
    Empty,
    /// A file's hunks, in original `git diff` order.
    File(Vec<AttributedHunk>),
}

/// The logical-line offset (before `crate::ui::wrap_lines`' width-based
/// wrapping — the same "one requested-scroll unit" `App::right_pane_scroll`
/// already operates in) where each hunk in `content` starts, in the exact
/// order `crate::ui::draw_diff_pane`/`diff_pane_lines` renders them — used
/// by `crate::run_app`'s `]c`/`[c` (`InputKey::NextHunk`/`PrevHunk`)
/// handling to jump the scroll offset to a hunk boundary.
///
/// Mirrors `diff_pane_lines`/`diff_pane_split_rows`'s own line-counting
/// exactly rather than reusing either function directly, since this module
/// must stay free of `ratatui` types (module doc comment) — a change to
/// either function's layout must be mirrored here by hand, the same trade
/// `crate::order`'s own doc comment already accepts for its deliberately
/// duplicated Tarjan SCC implementation.
pub fn hunk_start_lines(content: &DiffPaneContent) -> Vec<usize> {
    walk_hunks(content).map(|(_, start)| start).collect()
}

/// The logical-line offset (same "requested-scroll unit" [`hunk_start_lines`]
/// uses) to auto-scroll to when `symbol_range` is selected: the start of
/// the *first* hunk (original order) whose new-side extent intersects
/// `symbol_range`, via [`hunk_intersects`]' existing half-open rule.
/// Returns `None` when `content` is [`DiffPaneContent::Empty`] or no hunk
/// intersects — the caller falls back to leaving the scroll position
/// unchanged (equivalent to landing at the top of the already-fully-shown
/// file, ADR 0072's "nothing to scroll to is not a dead end" consequence).
///
/// Kept under its ADR 0027/0030 name for continuity — despite there no
/// longer being a "section" to start at, this is still the function
/// `crate::run_app` calls to resolve a symbol selection's auto-scroll
/// target.
pub fn section_start_line_for_symbol(
    content: &DiffPaneContent,
    symbol_range: LineRange,
) -> Option<usize> {
    let hunks = match content {
        DiffPaneContent::Empty => return None,
        DiffPaneContent::File(hunks) => hunks,
    };
    walk_hunks(content)
        .zip(hunks)
        .find(|((_, _), attributed)| {
            hunk_intersects(&attributed.hunk, symbol_range.start, symbol_range.end)
        })
        .map(|((_, start), _)| start)
}

/// The mirror image of [`section_start_line_for_symbol`] (ADR 0030): given
/// `scroll_line` (the same "requested-scroll unit" both that function and
/// [`hunk_start_lines`] use — [`crate::app::App::right_pane_scroll`]'s own
/// value), finds which hunk's rendered span `scroll_line` falls inside
/// (its header row through its last body row, inclusive; the *last* hunk's
/// span is open-ended, so an overscroll past the end of the content still
/// resolves to it rather than to nothing — mirroring
/// `symbol_id_for_scroll_line`'s own pre-ADR-0072 span rule) and returns
/// the id of the *first* symbol in `symbols` (source order) whose range
/// intersects that hunk, via [`hunk_intersects`]' existing half-open rule
/// — the same "first intersecting hunk/symbol" pairing
/// [`section_start_line_for_symbol`] uses in the opposite direction, so
/// the two stay round-trip consistent for a hunk owned by exactly one
/// symbol.
///
/// Returns `None` when: `content` is [`DiffPaneContent::Empty`]; or no
/// symbol's range intersects the hunk at `scroll_line` — `crate::run_app`'s
/// caller treats this as ADR 0030 decision 3 always has (leave the tree
/// cursor untouched rather than guess).
pub fn symbol_id_for_scroll_line<'a>(
    content: &DiffPaneContent,
    scroll_line: usize,
    symbols: &'a [(String, LineRange)],
) -> Option<&'a str> {
    let hunks = match content {
        DiffPaneContent::Empty => return None,
        DiffPaneContent::File(hunks) => hunks,
    };
    let starts: Vec<usize> = walk_hunks(content).map(|(_, start)| start).collect();
    let hunk_index = starts
        .iter()
        .enumerate()
        .rev()
        .find(|&(_, start)| *start <= scroll_line)?
        .0;
    let attributed = &hunks[hunk_index];
    symbols
        .iter()
        .find(|(_, range)| hunk_intersects(&attributed.hunk, range.start, range.end))
        .map(|(id, _)| id.as_str())
}

/// One entry per hunk for line-counting consumers ([`hunk_start_lines`],
/// [`section_start_line_for_symbol`], and [`symbol_id_for_scroll_line`] all
/// need the exact same layout walk, kept in one place so a change to
/// [`crate::ui::diff_pane::diff_pane_lines`]/[`crate::ui::diff_pane::diff_pane_split_rows`]'s
/// rendered layout only has to be mirrored once here). Yields
/// `(hunk_index, hunk_start_line)` — `hunk_start_line` is where the
/// hunk's own `@@` header line begins: a blank separator line precedes
/// every hunk but the first, then the header, then the hunk's body lines.
fn walk_hunks(content: &DiffPaneContent) -> impl Iterator<Item = (usize, usize)> {
    let hunks: &[AttributedHunk] = match content {
        DiffPaneContent::Empty => &[],
        DiffPaneContent::File(hunks) => hunks,
    };

    let mut line = 0usize;
    let mut out = Vec::with_capacity(hunks.len());
    for (index, attributed) in hunks.iter().enumerate() {
        if index > 0 {
            line += 1; // blank line between hunks
        }
        out.push((index, line));
        line += 1; // the hunk header line itself
        line += attributed.hunk.lines.len();
    }
    out.into_iter()
}

/// Builds the diff pane's shaped content for `target` (`None` mirrors
/// `App::selected_diff_target` returning `None` — nothing selected, or a
/// directory row). `diff_files` is the whole diff already parsed once by
/// `crate::run_app` (`crate::diff_view::parse_diff_hunks`), not re-parsed
/// here.
///
/// Per ADR 0072 both symbol-row and file-row selections produce the same
/// flat, original-order hunk list; a symbol selection is expressed by
/// `App::selected_diff_focus` (a separate accessor) and applied by
/// `crate::run_app` as an auto-scroll target, not by shaping the content
/// differently here.
pub fn build_diff_pane_content(
    _report: &Report,
    diff_files: &[FileHunks],
    target: Option<&DiffTarget>,
) -> DiffPaneContent {
    match target {
        None => DiffPaneContent::Empty,
        Some(DiffTarget::File { path }) => build_file_content(diff_files, path),
    }
}

fn build_file_content(diff_files: &[FileHunks], path: &str) -> DiffPaneContent {
    let Some(file_hunks) = file_hunks(diff_files, path) else {
        return DiffPaneContent::Empty;
    };
    if file_hunks.hunks.is_empty() {
        return DiffPaneContent::Empty;
    }

    let hunks = file_hunks
        .hunks
        .iter()
        .enumerate()
        .map(|(source_index, hunk)| AttributedHunk {
            source_index,
            hunk: hunk.clone(),
        })
        .collect();
    DiffPaneContent::File(hunks)
}

/// The distinct changed-line ranges across `hunks`, for the Diff pane
/// header's `range:` line ([`crate::ui::diff_pane::diff_pane_header_lines`]).
///
/// A pure-deletion hunk's `new_range` is a deliberately zero-width
/// `(start, start - 1)` (see [`crate::diff_view::Hunk::new_range`]'s own
/// doc comment) — excluded here, since there is no visible line span to
/// name a *range* for.
///
/// Sorted and deduped so a file whose hunks repeat a range (not possible
/// today since each hunk contributes at most one range, but cheap
/// insurance against a future caller passing the same hunk twice) still
/// produces one entry per distinct new-side span.
pub fn changed_line_ranges(hunks: &[&Hunk]) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = hunks
        .iter()
        .filter_map(|hunk| hunk.new_range)
        .filter(|(start, end)| start <= end)
        .collect();
    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

#[cfg(test)]
#[path = "diff_shape_tests/mod.rs"]
mod tests;
