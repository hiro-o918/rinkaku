//! Diff-pane content shaping (ADR 0020, ADR 0027): given the row currently
//! selected in the entry view (a symbol or a file) plus the already-parsed
//! diff hunks (`crate::diff_view`), decides how the diff pane groups and
//! annotates that content. Per ADR 0027, both symbol-row and file-row
//! selections now produce the same file-scoped shape: hunks grouped under
//! per-symbol section headers (unchanged from ADR 0020's file-selection
//! semantics), with each section carrying an optional `symbol_id` so
//! `crate::run_app` can look up the selected symbol's section start and
//! auto-scroll to it. The old `DiffPaneContent::Symbol` clip variant is gone
//! (ADR 0027 decision 1).
//!
//! Each section whose symbol's contract changed gets a 2-line old/new
//! signature header up front.
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
use crate::diff_view::{FileHunks, Hunk, file_hunks};
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

/// One [`Hunk`], cloned out of the original [`FileHunks`] into a shaped
/// [`DiffSection`] (this module's own doc comment on why cloning, not
/// borrowing), plus its `source_index` — its position in that original
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
/// handling to jump the scroll offset to a hunk boundary. Mirrors
/// `diff_pane_lines`'s own line-counting exactly (section separator blank
/// lines, an optional bold section header, an optional 2-line contract
/// header, a blank line before each hunk's own header when anything
/// precedes it) rather than reusing that function directly, since this
/// module must stay free of `ratatui` types (module doc comment) — a
/// change to `diff_pane_lines`'s layout must be mirrored here by hand, the
/// same trade `crate::order`'s own doc comment already accepts for its
/// deliberately duplicated Tarjan SCC implementation.
pub fn hunk_start_lines(content: &DiffPaneContent) -> Vec<usize> {
    let mut starts = Vec::new();
    for (_, _, _, hunk_starts) in walk_sections(content) {
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
/// Points at the *start of the section*, including its title line — not at
/// the first hunk's `@@` header (ADR 0027 decision 3): a reviewer moving
/// between symbols wants to see the section title (and its contract header,
/// when present) first, before the hunks that follow.
///
/// Used by `crate::run_app` to write the auto-scroll target into
/// `App::right_pane_scroll` right after `build_diff_pane_content` rebuilds
/// the shaped content for a new selection.
pub fn section_start_line_for_symbol(content: &DiffPaneContent, symbol_id: &str) -> Option<usize> {
    walk_sections(content)
        .find(|(_, section, _, _)| section.symbol_id.as_deref() == Some(symbol_id))
        .map(|(_, _, section_start, _)| section_start)
}

/// One entry per section for line-counting consumers ([`hunk_start_lines`]
/// and [`section_start_line_for_symbol`] both need the exact same layout
/// walk, kept in one place so a change to
/// [`crate::ui::diff_pane_lines`]'s rendered layout only has to be mirrored
/// once here — the same trade [`hunk_start_lines`]'s own doc comment already
/// accepts for the mirroring itself). Yields `(section_index, &section,
/// section_start_line, hunk_start_lines)` — `section_start_line` is where
/// the section's title (or its very first line, when nothing precedes it)
/// begins.
fn walk_sections(
    content: &DiffPaneContent,
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
        line += 1; // section title line (always shown now — ADR 0027)
        if section.contract_header.is_some() {
            line += 2; // "- previous" / "+ current" lines
        }

        let mut hunk_starts = Vec::with_capacity(section.hunks.len());
        for attributed in &section.hunks {
            // Blank line before every hunk header: the section title is
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

    for (source_index, hunk) in file_hunks.hunks.iter().enumerate() {
        // Every symbol (source order) whose range intersects this hunk —
        // ADR 0029 amends ADR 0020's original first-match-only rule: a
        // brand-new file's diff is always exactly one hunk spanning the
        // whole file, so first-match silently dropped every symbol but the
        // first from the diff pane and from auto-scroll (ADR 0027 decision
        // 2). The hunk is cloned once per matching section — see ADR 0029
        // for why the TUI departs from ADR 0020's "duplication misleads
        // about total change size" reasoning (the TUI has no change-size
        // total to mislead).
        let owners: Vec<usize> = symbols
            .iter()
            .enumerate()
            .filter(|(_, symbol)| {
                crate::diff_view::hunk_intersects(hunk, symbol.range.start, symbol.range.end)
            })
            .map(|(index, _)| index)
            .collect();
        if owners.is_empty() {
            module_level_hunks.push(AttributedHunk {
                source_index,
                hunk: hunk.clone(),
            });
        } else {
            for index in owners {
                sections[index].hunks.push(AttributedHunk {
                    source_index,
                    hunk: hunk.clone(),
                });
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;
    use rinkaku_core::render::{FileReport, ReportOrigin};

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
            origin: ReportOrigin::Diff,
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

    fn hunk(header: &str, new_range: Option<(usize, usize)>, lines: Vec<&str>) -> Hunk {
        Hunk {
            header: header.to_string(),
            new_range,
            lines: lines
                .into_iter()
                .map(|content| crate::diff_view::DiffLine {
                    kind: crate::diff_view::DiffLineKind::Context,
                    content: content.to_string(),
                })
                .collect(),
        }
    }

    /// Wraps `hunk` with the `source_index` it occupies in the fixture's
    /// `FileHunks::hunks` — every test below builds its `diff_files`
    /// fixture with hunks in a fixed order, so this index is just "which
    /// position in that `vec![...]` this hunk was written at".
    fn attributed(source_index: usize, hunk: Hunk) -> AttributedHunk {
        AttributedHunk { source_index, hunk }
    }

    #[test]
    fn should_return_empty_when_target_is_none() {
        let report = empty_report();

        let actual = build_diff_pane_content(&report, &[], None);

        assert_eq!(DiffPaneContent::Empty, actual);
    }

    #[test]
    fn should_group_file_selection_hunks_under_per_symbol_sections() {
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 }),
                    symbol("lib.rs::bar", "bar", LineRange { start: 10, end: 11 }),
                ],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
                hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
            ],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo()".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
                )],
            },
            DiffSection {
                title: "fn bar()".to_string(),
                symbol_id: Some("lib.rs::bar".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    1,
                    hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
                )],
            },
        ]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_attribute_pure_deletion_hunk_to_owning_symbol_instead_of_module_level() {
        // Finding-2 regression: `hunk_intersects` always returning `false`
        // for a pure-deletion hunk meant `build_file_content`'s owner lookup
        // (`symbols.iter().position(...)`) never matched, so every deletion
        // hunk landed in the `MODULE_LEVEL_TITLE` bucket regardless of which
        // symbol's body it actually came from.
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 4 })],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(
                "@@ -4 +3,0 @@",
                Some((3, 2)),
                vec!["println!(\"removed\");"],
            )],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk(
                    "@@ -4 +3,0 @@",
                    Some((3, 2)),
                    vec!["println!(\"removed\");"],
                ),
            )],
        }]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_bucket_hunk_under_module_level_when_it_intersects_no_symbol() {
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol(
                    "lib.rs::foo",
                    "foo",
                    LineRange { start: 10, end: 11 },
                )],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["use foo::bar;"]),
                hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn foo() {}"]),
            ],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo()".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    1,
                    hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn foo() {}"]),
                )],
            },
            DiffSection {
                title: MODULE_LEVEL_TITLE.to_string(),
                symbol_id: None,
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["use foo::bar;"]),
                )],
            },
        ]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_omit_module_level_section_when_every_hunk_is_attributed_to_a_symbol() {
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"])],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            )],
        }]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_attribute_overlapping_hunk_to_every_symbol_it_intersects() {
        // Two symbols with adjacent, overlapping ranges (a pathological
        // input a real extractor would not normally produce, but the
        // shaping function's contract must still resolve deterministically).
        // ADR 0029 amends ADR 0020's original first-match-only rule: a hunk
        // intersecting more than one symbol's range is now attributed to
        // every one of them, not just the first in source order — see ADR
        // 0029 for why the TUI diff pane departs from ADR 0020's
        // summary-view "duplication misleads about total change size"
        // reasoning (the TUI has no change-size total to mislead, and a
        // dropped section silently breaks that symbol's auto-scroll — ADR
        // 0027 decision 2 — which is the worse failure mode here).
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 5 }),
                    symbol("lib.rs::bar", "bar", LineRange { start: 3, end: 8 }),
                ],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk("@@ -1,1 +1,5 @@", Some((3, 4)), vec!["shared line"])],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo()".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,5 @@", Some((3, 4)), vec!["shared line"]),
                )],
            },
            DiffSection {
                title: "fn bar()".to_string(),
                symbol_id: Some("lib.rs::bar".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,5 @@", Some((3, 4)), vec!["shared line"]),
                )],
            },
        ]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_attribute_new_file_single_hunk_to_every_symbol_it_defines() {
        // Regression test (PR #86 dogfooding, ADR 0029): a brand-new file's
        // diff is always exactly one hunk spanning the whole file
        // (`@@ -0,0 +1,N @@`), so every symbol the file defines has a
        // range inside that one hunk. Before ADR 0029, only the first
        // symbol in source order (`foo`) ever got a section; `bar` and
        // `baz` were silently dropped, breaking their diff-pane auto-scroll
        // (ADR 0027 decision 2) with no error or indicator.
        let report = Report {
            files: vec![FileReport {
                path: "file_size.rs".to_string(),
                symbols: vec![
                    symbol("file_size.rs::foo", "foo", LineRange { start: 1, end: 3 }),
                    symbol("file_size.rs::bar", "bar", LineRange { start: 5, end: 7 }),
                    symbol("file_size.rs::baz", "baz", LineRange { start: 9, end: 11 }),
                ],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "file_size.rs".to_string(),
            hunks: vec![hunk(
                "@@ -0,0 +1,11 @@",
                Some((1, 11)),
                vec!["whole new file"],
            )],
        }];
        let target = DiffTarget::File {
            path: "file_size.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected_hunk = attributed(
            0,
            hunk("@@ -0,0 +1,11 @@", Some((1, 11)), vec!["whole new file"]),
        );
        let expected = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo()".to_string(),
                symbol_id: Some("file_size.rs::foo".to_string()),
                contract_header: None,
                hunks: vec![expected_hunk.clone()],
            },
            DiffSection {
                title: "fn bar()".to_string(),
                symbol_id: Some("file_size.rs::bar".to_string()),
                contract_header: None,
                hunks: vec![expected_hunk.clone()],
            },
            DiffSection {
                title: "fn baz()".to_string(),
                symbol_id: Some("file_size.rs::baz".to_string()),
                contract_header: None,
                hunks: vec![expected_hunk],
            },
        ]);
        assert_eq!(expected, actual);

        // Every symbol now resolves an auto-scroll target (ADR 0027
        // decision 2 / decision 4) — not just the first.
        assert_eq!(
            Some(0),
            section_start_line_for_symbol(&actual, "file_size.rs::foo")
        );
        assert!(section_start_line_for_symbol(&actual, "file_size.rs::bar").is_some());
        assert!(section_start_line_for_symbol(&actual, "file_size.rs::baz").is_some());
    }

    #[test]
    fn should_include_contract_header_on_the_owning_section_in_a_file_selection() {
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::SignatureChanged),
                    previous_signature: Some("fn foo(a: i32)".to_string()),
                    signature: "fn foo(a: i32, b: i32)".to_string(),
                    ..symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })
                }],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk(
                "@@ -1,1 +1,2 @@",
                Some((1, 2)),
                vec!["fn foo(a, b) {}"],
            )],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo(a: i32, b: i32)".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: Some(ContractHeader {
                previous_signature: "fn foo(a: i32)".to_string(),
                signature: "fn foo(a: i32, b: i32)".to_string(),
            }),
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
            )],
        }]);
        assert_eq!(expected, actual);
    }

    // Regression test (post-rebase integration check, PR #58): a skipped or
    // whole-test-file row (ADR: "show skipped and test-only files in the
    // entry tree") has no `FileReport` at all in `report.files`, so
    // `build_file_content`'s `symbols` lookup falls back to `&[]` — every
    // hunk must still land somewhere rather than being silently dropped or
    // panicking on an out-of-bounds `sections` index.
    #[test]
    fn should_bucket_every_hunk_under_module_level_when_file_selection_has_no_symbols_at_all() {
        // `report.files` has no entry for "assets/logo.png" at all — the
        // exact shape of a skipped/whole-test-file row, which is tracked in
        // `report.skipped`/`report.tests` instead of `report.files`.
        let report = empty_report();
        let diff_files = vec![FileHunks {
            path: "assets/logo.png".to_string(),
            hunks: vec![hunk(
                "@@ -1,1 +1,2 @@",
                Some((1, 2)),
                vec!["binary blob line"],
            )],
        }];
        let target = DiffTarget::File {
            path: "assets/logo.png".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::File(vec![DiffSection {
            title: MODULE_LEVEL_TITLE.to_string(),
            symbol_id: None,
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["binary blob line"]),
            )],
        }]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_when_file_has_no_hunks_at_all() {
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        assert_eq!(DiffPaneContent::Empty, actual);
    }

    #[test]
    fn should_return_empty_when_diff_has_no_entry_for_the_selected_file() {
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 2 })],
            }],
            ..empty_report()
        };
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };

        let actual = build_diff_pane_content(&report, &[], Some(&target));

        assert_eq!(DiffPaneContent::Empty, actual);
    }

    // Regression test (post-rebase integration check, PR #58): a binary
    // skipped file has a `FileHunks` entry (git still reports the path
    // touched a diff) but zero `@@` hunks in it ("Binary files ... differ"
    // has no hunk syntax for `crate::diff_view::parse_diff_hunks` to parse)
    // and no `FileReport`/symbols at all — the pane must degrade to `Empty`
    // (rendered by `crate::ui::draw_diff_pane` as its own placeholder text)
    // rather than panicking or fabricating a module-level section with no
    // hunks in it.
    #[test]
    fn should_return_empty_when_skipped_file_has_no_symbols_and_no_hunks() {
        let report = empty_report();
        let diff_files = vec![FileHunks {
            path: "assets/logo.png".to_string(),
            hunks: vec![],
        }];
        let target = DiffTarget::File {
            path: "assets/logo.png".to_string(),
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        assert_eq!(DiffPaneContent::Empty, actual);
    }

    // --- hunk_start_lines ---

    #[test]
    fn should_return_empty_hunk_starts_when_content_is_empty() {
        let actual = hunk_start_lines(&DiffPaneContent::Empty);

        assert_eq!(Vec::<usize>::new(), actual);
    }

    #[test]
    fn should_offset_first_hunk_start_by_title_and_blank_when_file_has_a_single_section_without_contract_header()
     {
        // ADR 0027 unified layout: the section title is always shown, and
        // every hunk (including the section's first) gets a blank line
        // before its header. So: title(0), blank(1), header(2) — the hunk
        // starts at line 2.
        let content = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk(
                    "@@ -1,1 +1,2 @@",
                    Some((1, 2)),
                    vec!["fn a() {}", "fn foo() {}"],
                ),
            )],
        }]);

        let actual = hunk_start_lines(&content);

        assert_eq!(vec![2], actual);
    }

    #[test]
    fn should_offset_hunk_start_by_contract_header_lines_when_section_has_one() {
        // Title(0), 2 contract-header lines (1, 2), blank before the hunk (3),
        // hunk header (4) — hunk starts at line 4.
        let content = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo(a, b)".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: Some(ContractHeader {
                previous_signature: "fn foo(a)".to_string(),
                signature: "fn foo(a, b)".to_string(),
            }),
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
            )],
        }]);

        let actual = hunk_start_lines(&content);

        assert_eq!(vec![4], actual);
    }

    #[test]
    fn should_offset_second_hunk_start_by_first_hunk_header_and_body_length() {
        // Section title(0), blank(1), first hunk header(2), 2 body lines(3,4),
        // blank before second hunk(5), second hunk header(6) — starts at 2, 6.
        let content = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![
                attributed(
                    0,
                    hunk(
                        "@@ -1,1 +1,2 @@",
                        Some((1, 2)),
                        vec!["fn a() {}", "fn b() {}"],
                    ),
                ),
                attributed(
                    1,
                    hunk("@@ -10,1 +11,1 @@", Some((11, 11)), vec!["fn c() {}"]),
                ),
            ],
        }]);

        let actual = hunk_start_lines(&content);

        assert_eq!(vec![2, 6], actual);
    }

    #[test]
    fn should_offset_hunk_start_by_section_header_and_separator_lines_for_a_file_selection() {
        // Section 0: title(0), blank(1), header(2), 1 body line(3) — hunk
        // starts at line 2. Section 1: blank separator between sections(4),
        // title(5), blank before its hunk(6), header(7) — hunk starts at 7.
        let content = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo()".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
                )],
            },
            DiffSection {
                title: "fn bar()".to_string(),
                symbol_id: Some("lib.rs::bar".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    1,
                    hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
                )],
            },
        ]);

        let actual = hunk_start_lines(&content);

        assert_eq!(vec![2, 7], actual);
    }

    #[test]
    fn should_emit_one_hunk_jump_stop_per_section_when_a_hunk_is_shared_by_two_symbols() {
        // ADR 0029 consequence: a hunk attributed to more than one symbol
        // (an overlapping-range case) is rendered once per owning section,
        // so `]c`/`[c` (backed by this table) must stop once per rendered
        // occurrence — matching what is actually on screen — rather than
        // deduplicating by the shared `source_index` down to one stop.
        // Built straight from `build_diff_pane_content`'s real output
        // (not a hand-built `DiffPaneContent`) so this test exercises the
        // same shape the overlapping-hunk attribution test above produces.
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![
                    symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 5 }),
                    symbol("lib.rs::bar", "bar", LineRange { start: 3, end: 8 }),
                ],
            }],
            ..empty_report()
        };
        let diff_files = vec![FileHunks {
            path: "lib.rs".to_string(),
            hunks: vec![hunk("@@ -1,1 +1,5 @@", Some((3, 4)), vec!["shared line"])],
        }];
        let target = DiffTarget::File {
            path: "lib.rs".to_string(),
        };
        let content = build_diff_pane_content(&report, &diff_files, Some(&target));

        let actual = hunk_start_lines(&content);

        // Section 0 (`foo`): title(0), blank(1), hunk header(2), 1 body
        // line(3) — 4 lines. Blank separator(4), section 1 (`bar`)
        // title(5), blank(6), hunk header(7) — stop at 7. Two stops for
        // the one underlying hunk, one per section it was duplicated into.
        assert_eq!(vec![2, 7], actual);
    }

    // --- section_start_line_for_symbol ---

    #[test]
    fn should_return_none_for_symbol_start_when_content_is_empty() {
        let actual = section_start_line_for_symbol(&DiffPaneContent::Empty, "lib.rs::foo");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_zero_for_symbol_start_when_content_has_a_single_matching_section() {
        // Only section: its title is at line 0, so the section starts at 0.
        let content = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            )],
        }]);

        let actual = section_start_line_for_symbol(&content, "lib.rs::foo");

        assert_eq!(Some(0), actual);
    }

    #[test]
    fn should_return_second_section_start_when_symbol_id_matches_the_second_section() {
        // Section 0 layout: title(0), blank(1), header(2), body(3) — 4 lines.
        // Blank separator between sections at line 4, so section 1 starts at 5.
        let content = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo()".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
                )],
            },
            DiffSection {
                title: "fn bar()".to_string(),
                symbol_id: Some("lib.rs::bar".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    1,
                    hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
                )],
            },
        ]);

        let actual = section_start_line_for_symbol(&content, "lib.rs::bar");

        assert_eq!(Some(5), actual);
    }

    #[test]
    fn should_point_at_section_title_line_when_section_has_a_contract_header() {
        // The section start is the title line, *before* the contract header —
        // ADR 0027 decision 3: the reviewer wants the section title and its
        // contract change first, not the hunks below them.
        // Section 0: title(0), body(1), 2 contract-header lines(2,3), blank
        // before hunk(4), hunk header(5) — 6 lines. Blank between sections at
        // line 6, section 1 title at line 7.
        let content = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo(a, b)".to_string(),
                symbol_id: Some("lib.rs::foo".to_string()),
                contract_header: Some(ContractHeader {
                    previous_signature: "fn foo(a)".to_string(),
                    signature: "fn foo(a, b)".to_string(),
                }),
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
                )],
            },
            DiffSection {
                title: "fn bar()".to_string(),
                symbol_id: Some("lib.rs::bar".to_string()),
                contract_header: None,
                hunks: vec![attributed(
                    1,
                    hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn bar() {}"]),
                )],
            },
        ]);

        let actual = section_start_line_for_symbol(&content, "lib.rs::bar");

        assert_eq!(Some(7), actual);
    }

    #[test]
    fn should_return_none_for_module_level_bucket_when_asked_by_any_symbol_id() {
        // The module-level bucket has `symbol_id: None`, so no real symbol id
        // lookup can accidentally match it. Even passing the literal
        // `MODULE_LEVEL_TITLE` as a symbol id (which is not a valid symbol id
        // shape but is the closest a caller could get to "aim at the bucket")
        // must not match.
        let content = DiffPaneContent::File(vec![DiffSection {
            title: MODULE_LEVEL_TITLE.to_string(),
            symbol_id: None,
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["use foo::bar;"]),
            )],
        }]);

        let actual = section_start_line_for_symbol(&content, MODULE_LEVEL_TITLE);

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_none_when_symbol_id_matches_no_section() {
        let content = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo()".to_string(),
            symbol_id: Some("lib.rs::foo".to_string()),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            )],
        }]);

        let actual = section_start_line_for_symbol(&content, "lib.rs::nonexistent");

        assert_eq!(None, actual);
    }
}
