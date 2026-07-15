//! Diff-pane content shaping (ADR 0020, ADR 0027, ADR 0030): given the row
//! currently selected in the entry view (a symbol or a file) plus the
//! already-parsed diff hunks (`crate::diff_view`), decides how the diff
//! pane groups and annotates that content. Per ADR 0027, both symbol-row
//! and file-row selections now produce the same file-scoped shape: hunks
//! grouped under per-symbol section headers (unchanged from ADR 0020's
//! file-selection semantics), with each section carrying an optional
//! `symbol_id` so `crate::run_app` can look up the selected symbol's
//! section start and auto-scroll to it. The old `DiffPaneContent::Symbol`
//! clip variant is gone (ADR 0027 decision 1). ADR 0030 adds the mirror
//! image — [`symbol_id_for_scroll_line`] resolves a scroll offset back to
//! the symbol whose section it falls inside, so `crate::run_app` can sync
//! the tree cursor when the reviewer scrolls the pane manually.
//!
//! Each section whose symbol's contract changed carries a
//! [`ContractHeader`], rendered by `crate::ui::diff_pane` in place of the
//! section's own title line (the anchor row(s) a reviewer lands on when
//! jumping to that symbol) rather than as a separate header.
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

use crate::app::{DiffTarget, DiffViewMode};
use crate::diff_view::{FileHunks, Hunk, file_hunks};
pub use crate::split_pairing::{SplitRow, pair_hunk_lines};
use rinkaku_core::extract::Classification;
use rinkaku_core::render::Report;

/// A 2-line old/new signature header, shown before a symbol's hunks when
/// [`rinkaku_core::extract::ExtractedSymbol::previous_signature`] is
/// `Some` — the diff pane's outline-then-implementation disclosure order
/// (ADR 0020): the reader sees *that* the contract changed, and to what,
/// before the hunks showing *how*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractHeader {
    pub previous_signature: String,
    pub signature: String,
}

/// One [`Hunk`] (or, per ADR 0053, one per-symbol slice of a shared hunk —
/// see `origin_offset` below), cloned out of the original [`FileHunks`] into
/// a shaped [`DiffSection`] (this module's own doc comment on why cloning,
/// not borrowing), plus its `source_index` — its position in that original
/// `FileHunks::hunks` slice. `crate::highlight::lookup_hunk_highlight`
/// looks up a hunk's precomputed highlight by `std::ptr::eq` against the
/// *original* `Hunk` it was highlighted from; a clone breaks that pointer
/// identity, so `source_index` is threaded through instead — `crate::ui`
/// uses it to index straight into the original `FileHunks`/`HighlightedFile`
/// rather than re-deriving position via a fragile equality search (a file
/// can legitimately contain two textually-identical hunks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributedHunk {
    pub source_index: usize,
    pub hunk: Hunk,
    /// This hunk's start index within the *original* hunk's `lines`
    /// (`crate::diff_shape::AttributedHunk`), i.e. `hunk_split::SubHunk::origin_offset`
    /// (ADR 0053) — `0` for a hunk that was never split (the common case,
    /// including every hunk owned by only one symbol). `crate::ui::diff_pane`
    /// offsets a line's position within `hunk.lines` by this before indexing
    /// into `crate::highlight::lookup_hunk_highlight_by_index`'s
    /// `source_index`-keyed, *original*-hunk-length highlight slice — the
    /// highlight table is still computed once per original hunk (highlighting
    /// is the expensive step; ADR 0053 does not want to re-run it once per
    /// sub-hunk for the same tokens), so a sub-hunk's own `lines` no longer
    /// line up positionally with that table without this offset.
    pub origin_offset: usize,
}

/// One symbol's worth of shaped diff content: its own contract header
/// (when its signature changed) followed by the hunks attributed to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSection {
    /// The section header text — a symbol's own signature line, or the
    /// fixed `"(module level)"` label for hunks intersecting no symbol at
    /// all (import-only changes, module-level `use` statements).
    pub title: String,
    /// The extracted symbol's id (matches
    /// [`rinkaku_core::extract::ExtractedSymbol::id`] for symbol sections,
    /// `None` for the module-level bucket) — used by `crate::run_app` to
    /// find "which section is the selected symbol's" and auto-scroll the
    /// diff pane's `right_pane_scroll` to that section's start (ADR 0027
    /// decision 2). Kept as an `Option` rather than a separate lookup on
    /// `Report` so the lookup stays a plain `iter().find()` over already-
    /// shaped sections, and so the module-level bucket cannot accidentally
    /// match any real symbol id.
    pub symbol_id: Option<String>,
    pub contract_header: Option<ContractHeader>,
    pub hunks: Vec<AttributedHunk>,
}

/// The diff pane's fully shaped content for the current selection —
/// what `crate::ui::draw_diff_pane` renders, computed once by
/// `crate::run_app` and handed in rather than recomputed per draw.
///
/// Per ADR 0027 decision 1 the old `Symbol(DiffSection)` clip variant was
/// removed: both symbol-row and file-row selections now produce `File(..)`,
/// and a symbol selection is expressed as an auto-scroll target (see
/// [`section_start_line_for_symbol`]) rather than a distinct pane shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffPaneContent {
    /// Nothing to show: no row selected, a directory row (no diff-specific
    /// content of its own — `App::selected_diff_target`'s own doc comment),
    /// or (defensively) a mismatch between `report` and the diff text.
    Empty,
    /// A file's hunks, grouped into one [`DiffSection`] per symbol in
    /// `report.files[..].symbols` order, plus a trailing `"(module level)"`
    /// section for hunks intersecting no symbol at all — omitted (not an
    /// empty section) when every hunk was attributed to some symbol.
    File(Vec<DiffSection>),
}

/// The fixed label for a file selection's trailing section of hunks that
/// intersect no symbol's line range (import-only changes, module-level
/// `use` statements) — not a symbol's own signature, so it cannot reuse
/// [`DiffSection::title`]'s symbol-signature convention.
pub const MODULE_LEVEL_TITLE: &str = "(module level)";

/// The logical-line offset (before `crate::ui::wrap_lines`' width-based
/// wrapping — the same "one requested-scroll unit" `App::right_pane_scroll`
/// already operates in) where each hunk in `content` starts, in the exact
/// order `crate::ui::draw_diff_pane`/`diff_pane_lines` renders them — used
/// by `crate::run_app`'s `]c`/`[c` (`InputKey::NextHunk`/`PrevHunk`)
/// handling to jump the scroll offset to a hunk boundary. `view_mode` picks
/// which of the two anchor-row shapes [`walk_sections`] counts (unified's
/// title-or-2-line-signature-pair vs. split's always-one-row anchor —
/// [`walk_sections`]'s own doc comment); callers pass the *effective* mode
/// last drawn ([`crate::ui::DrawOutcome::effective_diff_view_mode`], folded
/// back into `crate::run_app`'s loop between frames), not the requested
/// `App::diff_view_mode()`, so the count matches what a reviewer actually
/// sees when ADR 0044 decision 7's [`crate::ui::diff_pane::MIN_SPLIT_VIEW_WIDTH`]
/// fallback silently renders `Split` as `Unified`.
/// Mirrors `diff_pane_lines`/`diff_pane_split_rows`'s own line-counting
/// exactly rather than reusing either function directly, since this module
/// must stay free of `ratatui` types (module doc comment) — a change to
/// either function's layout must be mirrored here by hand, the same trade
/// `crate::order`'s own doc comment already accepts for its deliberately
/// duplicated Tarjan SCC implementation.
pub fn hunk_start_lines(content: &DiffPaneContent, view_mode: DiffViewMode) -> Vec<usize> {
    let mut starts = Vec::new();
    for (_, _, _, hunk_starts) in walk_sections(content, view_mode) {
        starts.extend(hunk_starts);
    }
    starts
}

/// The logical-line offset (same "requested-scroll unit"
/// [`hunk_start_lines`] uses) where the section whose `symbol_id` matches
/// `symbol_id` starts, in the exact order [`crate::ui::draw_diff_pane`]
/// renders sections. Returns `None` when `content` is
/// [`DiffPaneContent::Empty`] or no section matches (the selected symbol
/// contributed no hunks of its own, e.g. a `BodyOnly` classification whose
/// diff lines all fell inside another symbol's range — same rule as
/// [`build_diff_pane_content`]'s "drop empty sections" step).
///
/// Points at the *start of the section*, including its anchor row(s) — not
/// at the first hunk's `@@` header (ADR 0027 decision 3): a reviewer moving
/// between symbols wants to see the section's title or changed-signature
/// pair first, before the hunks that follow. `view_mode` has the same
/// meaning as [`hunk_start_lines`]'s own parameter.
///
/// Used by `crate::run_app` to write the auto-scroll target into
/// `App::right_pane_scroll` right after `build_diff_pane_content` rebuilds
/// the shaped content for a new selection.
pub fn section_start_line_for_symbol(
    content: &DiffPaneContent,
    symbol_id: &str,
    view_mode: DiffViewMode,
) -> Option<usize> {
    walk_sections(content, view_mode)
        .find(|(_, section, _, _)| section.symbol_id.as_deref() == Some(symbol_id))
        .map(|(_, _, section_start, _)| section_start)
}

/// The mirror image of [`section_start_line_for_symbol`] (ADR 0030): given
/// `scroll_line` (the same "requested-scroll unit" both that function and
/// [`hunk_start_lines`] use — [`crate::app::App::right_pane_scroll`]'s own
/// value), finds which section's rendered span `scroll_line` falls inside
/// and returns that section's `symbol_id`. A section's span runs from its
/// own start line (inclusive) up to the next section's start line
/// (exclusive), or through the end of the content for the last section —
/// so scrolling anywhere within a symbol's anchor row(s)/hunks resolves to
/// that symbol, not just its exact first line. `view_mode` has the same
/// meaning as [`hunk_start_lines`]'s own parameter.
///
/// Returns `None` in two cases `crate::run_app`'s caller treats
/// identically (ADR 0030 decision 3 — leave the tree cursor untouched
/// rather than guess): `scroll_line` falls inside the
/// `"(module level)"` bucket (`DiffSection::symbol_id: None` by
/// construction, same as [`section_start_line_for_symbol`]'s own
/// module-level exclusion), or `scroll_line` is past the end of every
/// section (an overscroll about to be clamped by `crate::ui::clamp_scroll`
/// next frame) — also `None` on [`DiffPaneContent::Empty`].
pub fn symbol_id_for_scroll_line(
    content: &DiffPaneContent,
    scroll_line: usize,
    view_mode: DiffViewMode,
) -> Option<&str> {
    let sections: Vec<(usize, &DiffSection, usize, Vec<usize>)> =
        walk_sections(content, view_mode).collect();
    let (_, (_, section, _, _)) =
        sections
            .iter()
            .enumerate()
            .find(|(index, (_, _, start, _))| {
                let next_start = sections
                    .get(index + 1)
                    .map(|(_, _, next_start, _)| *next_start);
                match next_start {
                    Some(next_start) => (*start..next_start).contains(&scroll_line),
                    None => scroll_line >= *start,
                }
            })?;
    section.symbol_id.as_deref()
}

/// One entry per section for line-counting consumers ([`hunk_start_lines`]
/// and [`section_start_line_for_symbol`] both need the exact same layout
/// walk, kept in one place so a change to
/// [`crate::ui::diff_pane_lines`]/[`crate::ui::diff_pane_split_rows`]'s
/// rendered layout only has to be mirrored once here — the same trade
/// [`hunk_start_lines`]'s own doc comment already accepts for the
/// mirroring itself). Yields `(section_index, &section, section_start_line,
/// hunk_start_lines)` — `section_start_line` is where the section's anchor
/// row (or its very first line, when nothing precedes it) begins.
///
/// `view_mode` picks the anchor's row count: unified
/// ([`DiffViewMode::Unified`]) renders a changed signature as 2 stacked
/// rows but an unchanged title as 1; split ([`DiffViewMode::Split`]) always
/// pairs the anchor onto exactly 1 row since old/new sit side by side
/// rather than stacked — the two modes' row counts only diverge on a
/// section whose signature changed, and only there.
fn walk_sections(
    content: &DiffPaneContent,
    view_mode: DiffViewMode,
) -> impl Iterator<Item = (usize, &DiffSection, usize, Vec<usize>)> {
    let sections: &[DiffSection] = match content {
        DiffPaneContent::Empty => &[],
        DiffPaneContent::File(sections) => sections,
    };

    let mut line = 0usize;
    let mut out = Vec::with_capacity(sections.len());
    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            line += 1; // blank line between sections
        }
        let section_start = line;
        line += match (view_mode, section.contract_header.is_some()) {
            (DiffViewMode::Split, _) => 1,
            (DiffViewMode::Unified, true) => 2,
            (DiffViewMode::Unified, false) => 1,
        };

        let mut hunk_starts = Vec::with_capacity(section.hunks.len());
        for attributed in &section.hunks {
            // Blank line before every hunk header: the section anchor is
            // always shown (ADR 0027 collapsed the two former
            // `show_section_headers` cases into one), so every hunk —
            // including a section's first — has *something* on the line
            // above it and needs the visual separator.
            line += 1;
            hunk_starts.push(line);
            line += 1; // the hunk header line itself
            line += attributed.hunk.lines.len();
        }
        out.push((section_index, section, section_start, hunk_starts));
    }
    out.into_iter()
}

/// Builds the diff pane's shaped content for `target` (`None` mirrors
/// `App::selected_diff_target` returning `None` — nothing selected, or a
/// directory row). `diff_files` is the whole diff already parsed once by
/// `crate::run_app` (`crate::diff_view::parse_diff_hunks`), not re-parsed
/// here.
///
/// Per ADR 0027 both symbol-row and file-row selections produce the same
/// file-scoped shape; a symbol selection is expressed by
/// `App::selected_diff_focus` (a separate accessor) and applied by
/// `crate::run_app` as an auto-scroll target, not by returning a different
/// `DiffPaneContent` variant here.
pub fn build_diff_pane_content(
    report: &Report,
    diff_files: &[FileHunks],
    target: Option<&DiffTarget>,
) -> DiffPaneContent {
    match target {
        None => DiffPaneContent::Empty,
        Some(DiffTarget::File { path }) => build_file_content(report, diff_files, path),
    }
}

fn build_file_content(report: &Report, diff_files: &[FileHunks], path: &str) -> DiffPaneContent {
    let Some(file_hunks) = file_hunks(diff_files, path) else {
        return DiffPaneContent::Empty;
    };
    if file_hunks.hunks.is_empty() {
        return DiffPaneContent::Empty;
    }

    let symbols = report
        .files
        .iter()
        .find(|file| file.path == path)
        .map(|file| file.symbols.as_slice())
        .unwrap_or(&[]);

    let mut sections: Vec<DiffSection> = symbols
        .iter()
        .map(|symbol| DiffSection {
            title: symbol.signature.clone(),
            symbol_id: Some(symbol.id.clone()),
            contract_header: contract_header_for_symbol(symbol),
            hunks: Vec::new(),
        })
        .collect();
    let mut module_level_hunks: Vec<AttributedHunk> = Vec::new();

    let symbol_ranges: Vec<(usize, rinkaku_core::diff::LineRange)> = symbols
        .iter()
        .enumerate()
        .map(|(index, symbol)| (index, symbol.range))
        .collect();

    for (source_index, hunk) in file_hunks.hunks.iter().enumerate() {
        // Every symbol (source order) whose range intersects this hunk —
        // ADR 0029 amends ADR 0020's original first-match-only rule: a
        // brand-new file's diff is always exactly one hunk spanning the
        // whole file, so first-match silently dropped every symbol but the
        // first from the diff pane and from auto-scroll (ADR 0027 decision
        // 2). ADR 0053 further amends the attribution step itself: a hunk
        // shared by more than one symbol is split into per-symbol
        // sub-hunks (`crate::hunk_split::split_hunk`) rather than cloned
        // whole into every owning section, so a reviewer no longer reads
        // the same hunk body once per owning symbol.
        for (owner, sub_hunk) in crate::hunk_split::split_hunk(hunk, &symbol_ranges) {
            let attributed = AttributedHunk {
                source_index,
                hunk: Hunk {
                    header: sub_hunk.header,
                    new_range: sub_hunk.new_range,
                    lines: sub_hunk.lines,
                },
                origin_offset: sub_hunk.origin_offset,
            };
            match owner {
                Some(index) => sections[index].hunks.push(attributed),
                None => module_level_hunks.push(attributed),
            }
        }
    }

    // Symbols with no hunks of their own (e.g. a `BodyOnly` classification
    // whose diff lines all fell inside another symbol's range in a
    // pathological overlap, or simply a symbol this hunk-owner walk never
    // matched) contribute an empty, content-free section — dropped rather
    // than shown blank, since a heading with nothing under it adds noise
    // without adding information for a reviewer scanning the file's diff.
    sections.retain(|section| !section.hunks.is_empty());

    if !module_level_hunks.is_empty() {
        sections.push(DiffSection {
            title: MODULE_LEVEL_TITLE.to_string(),
            symbol_id: None,
            contract_header: None,
            hunks: module_level_hunks,
        });
    }

    if sections.is_empty() {
        return DiffPaneContent::Empty;
    }

    DiffPaneContent::File(sections)
}

/// The 2-line contract header for `symbol`, or `None` when its contract
/// did not change (or — defensively, "should not happen" per
/// `classify_symbols`'s contract — `previous_signature` is missing despite
/// `SignatureChanged`, mirroring `crate::detail::build_detail`'s identical
/// fallback).
fn contract_header_for_symbol(
    symbol: &rinkaku_core::extract::ExtractedSymbol,
) -> Option<ContractHeader> {
    match (symbol.classification, &symbol.previous_signature) {
        (Some(Classification::SignatureChanged), Some(previous)) => Some(ContractHeader {
            previous_signature: previous.clone(),
            signature: symbol.signature.clone(),
        }),
        _ => None,
    }
}

/// The distinct changed-line ranges across `sections`' hunks, for the
/// Diff pane header's `range:` line
/// ([`crate::ui::diff_pane::diff_pane_header_lines`]) — `sections` is
/// already the exact slice the caller is about to render, so this only
/// folds over `AttributedHunk`s already in hand.
///
/// A pure-deletion hunk's `new_range` is a deliberately zero-width
/// `(start, start - 1)` (see [`crate::diff_view::Hunk::new_range`]'s own
/// doc comment) — excluded here, since there is no visible line span to
/// name a *range* for.
///
/// Sorted and deduped so a file selection whose hunks ADR 0029 clones
/// across multiple owning symbols produces one entry per distinct
/// new-side span, not one per section that owns it (the tree's own
/// `chg:` badge already counts changed symbols; the ranges line reports
/// changed *lines*).
pub fn changed_line_ranges(sections: &[&DiffSection]) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = sections
        .iter()
        .flat_map(|section| &section.hunks)
        .filter_map(|attributed| attributed.hunk.new_range)
        .filter(|(start, end)| start <= end)
        .collect();
    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

#[cfg(test)]
#[path = "diff_shape_tests/mod.rs"]
mod tests;
