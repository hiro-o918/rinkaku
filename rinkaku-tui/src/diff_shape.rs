//! Diff-pane content shaping (ADR 0020, ADR 0027, ADR 0030, ADR 0072, ADR
//! 0074): given the row currently selected in the entry view (a symbol or a
//! file) plus the already-parsed diff hunks (`crate::diff_view`), decides
//! what the diff pane shows. Per ADR 0072 both symbol-row and file-row
//! selections show the same content — the whole file's hunks, in original
//! `git diff` order, with no per-symbol grouping; a symbol selection only
//! changes where the pane auto-scrolls to
//! ([`scroll_target_line_for_symbol`]). ADR 0030 adds the mirror image —
//! [`symbol_id_for_scroll_line`] resolves a scroll offset back to the symbol
//! owning the row at that offset, so `crate::run_app` can sync the tree
//! cursor when the reviewer scrolls the pane manually.
//!
//! Both directions resolve at *row* granularity, not whole-hunk granularity
//! (ADR 0074): every rendered row carries the new-side line coordinate it
//! sits at (`diff_rows`), so the several symbols a single large hunk
//! commonly covers each get their own distinct scroll target instead of
//! collapsing onto that hunk's header row.
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
use crate::diff_view::{FileHunks, Hunk, file_hunks, hunk_header_position, new_side_positions};
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
/// initial scroll target ([`scroll_target_line_for_symbol`]), not in what
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

/// One rendered row of the diff pane's scrollable body, in the exact order
/// `crate::ui::diff_pane`'s `diff_pane_lines`/`diff_pane_split_rows`
/// emit them — the unit `crate::app::App::right_pane_scroll` counts in
/// (ADR 0052: logical lines, before `crate::ui::wrap_lines`' width-based
/// wrapping).
///
/// Each variant carries the *new-side line coordinate* the row sits at
/// ([`crate::diff_view::new_side_positions`]), which is what makes both
/// scroll-sync directions row-precise rather than hunk-precise (ADR 0074).
/// `None` where there is no coordinate to carry: a separator row belongs to
/// no hunk, and a hunk whose header this parser could not read has no
/// new-side start to count from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffRow {
    /// The blank line rendered between two hunks.
    Separator,
    /// A hunk's own `@@` header row.
    Header(Option<usize>),
    /// One of a hunk's body rows.
    Body(Option<usize>),
}

impl DiffRow {
    fn new_side_position(self) -> Option<usize> {
        match self {
            DiffRow::Separator => None,
            DiffRow::Header(position) | DiffRow::Body(position) => position,
        }
    }
}

/// Every rendered row of `content`'s scrollable body, indexed by the same
/// logical-line offset `App::right_pane_scroll` holds — the one layout walk
/// [`hunk_start_lines`], [`scroll_target_line_for_symbol`], and
/// [`symbol_id_for_scroll_line`] all resolve against, kept in one place so a
/// change to the rendered layout only has to be mirrored here once.
///
/// Mirrors `diff_pane_lines`/`diff_pane_split_rows`'s own line-counting by
/// hand rather than reusing either function, since this module must stay
/// free of `ratatui` types (module doc comment) — the same trade
/// `crate::order`'s own doc comment already accepts for its deliberately
/// duplicated Tarjan SCC implementation.
fn diff_rows(content: &DiffPaneContent) -> Vec<DiffRow> {
    let hunks: &[AttributedHunk] = match content {
        DiffPaneContent::Empty => &[],
        DiffPaneContent::File(hunks) => hunks,
    };

    let mut rows = Vec::new();
    for (index, attributed) in hunks.iter().enumerate() {
        if index > 0 {
            rows.push(DiffRow::Separator);
        }
        rows.push(DiffRow::Header(hunk_header_position(&attributed.hunk)));
        rows.extend(
            new_side_positions(&attributed.hunk)
                .into_iter()
                .map(DiffRow::Body),
        );
    }
    rows
}

/// The logical-line offset (before `crate::ui::wrap_lines`' width-based
/// wrapping — the same "one requested-scroll unit" `App::right_pane_scroll`
/// already operates in) where each hunk in `content` starts, in the exact
/// order `crate::ui::draw_diff_pane`/`diff_pane_lines` renders them — used
/// by `crate::run_app`'s `]c`/`[c` (`InputKey::NextHunk`/`PrevHunk`)
/// handling to jump the scroll offset to a hunk boundary.
pub fn hunk_start_lines(content: &DiffPaneContent) -> Vec<usize> {
    diff_rows(content)
        .iter()
        .enumerate()
        .filter(|(_, row)| matches!(row, DiffRow::Header(_)))
        .map(|(line, _)| line)
        .collect()
}

/// The logical-line offset (same "requested-scroll unit" [`hunk_start_lines`]
/// uses) to auto-scroll to when `symbol_range` is selected: the *first*
/// rendered row (render order) whose own new-side coordinate falls inside
/// `symbol_range`.
///
/// Row-precise rather than hunk-precise (ADR 0074): a single hunk routinely
/// covers several symbols — a whole new file arrives as one hunk, and any
/// hunk with generous context spans its neighbours — so returning the
/// enclosing hunk's header row made every symbol under that hunk share one
/// target, and moving the tree cursor between them scrolled the pane
/// nowhere. Resolving the row instead gives each symbol the offset its own
/// lines start at. A symbol that starts exactly where its hunk does still
/// resolves to that hunk's header row, since the header shares its first
/// body line's coordinate ([`crate::diff_view::hunk_header_position`]) —
/// the `@@` line stays on screen wherever it is genuinely the start of what
/// was selected.
///
/// Because a `Removed` row carries the line it immediately *precedes*
/// ([`crate::diff_view::new_side_positions`]), a changed signature's `-`
/// line resolves to the same coordinate as the `+` line replacing it, so
/// the target lands on the first of the pair rather than one row past it.
///
/// Returns `None` when `content` is [`DiffPaneContent::Empty`] or no row
/// falls inside `symbol_range` — the caller falls back to leaving the
/// scroll position unchanged (equivalent to landing at the top of the
/// already-fully-shown file, ADR 0072's "nothing to scroll to is not a
/// dead end" consequence).
pub fn scroll_target_line_for_symbol(
    content: &DiffPaneContent,
    symbol_range: LineRange,
) -> Option<usize> {
    diff_rows(content)
        .iter()
        .position(|row| row_is_within(*row, symbol_range))
}

/// The mirror image of [`scroll_target_line_for_symbol`] (ADR 0030): given
/// `scroll_line` (the same "requested-scroll unit" both that function and
/// [`hunk_start_lines`] use — [`crate::app::App::right_pane_scroll`]'s own
/// value), resolves the row at that offset to its new-side coordinate and
/// returns the id of the *first* symbol in `symbols` (source order) whose
/// range contains it — the same row-precise coordinate
/// [`scroll_target_line_for_symbol`] targets in the opposite direction, so
/// the two stay round-trip consistent (ADR 0074).
///
/// A row with no coordinate of its own (the blank separator between two
/// hunks, or a hunk whose header this parser could not read) resolves to
/// the nearest preceding row that has one, so a scroll position parked on a
/// separator still belongs to the hunk above it. An overscroll past the last
/// row clamps to that row rather than resolving to nothing, preserving ADR
/// 0030 decision 3's open-ended span for the final hunk.
///
/// Returns `None` when: `content` is [`DiffPaneContent::Empty`]; no row at
/// or before `scroll_line` carries a coordinate; or no symbol's range
/// contains it — `crate::run_app`'s caller treats all three as ADR 0030
/// decision 3 always has (leave the tree cursor untouched rather than
/// guess).
pub fn symbol_id_for_scroll_line<'a>(
    content: &DiffPaneContent,
    scroll_line: usize,
    symbols: &'a [(String, LineRange)],
) -> Option<&'a str> {
    let rows = diff_rows(content);
    let last_row = rows.len().checked_sub(1)?;
    let position = rows[..=scroll_line.min(last_row)]
        .iter()
        .rev()
        .find_map(|row| row.new_side_position())?;
    symbols
        .iter()
        .find(|(_, range)| range.start <= position && position <= range.end)
        .map(|(id, _)| id.as_str())
}

/// Whether `row`'s own new-side coordinate falls inside `range`
/// (1-based inclusive, [`LineRange`]'s own convention). A row with no
/// coordinate belongs to no symbol.
fn row_is_within(row: DiffRow, range: LineRange) -> bool {
    row.new_side_position()
        .is_some_and(|position| range.start <= position && position <= range.end)
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
