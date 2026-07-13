//! Diff-pane content shaping (ADR 0020): given the row currently selected
//! in the entry view (a symbol or a file) plus the already-parsed diff
//! hunks (`crate::diff_view`), decides how the diff pane groups and
//! annotates that content — a symbol selection clips to its own hunks
//! (unchanged from before this ADR), while a file selection now groups
//! hunks under per-symbol section headers instead of listing them
//! undifferentiated, and either selection gets a 2-line old/new signature
//! header up front when the symbol's contract changed.
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
    pub contract_header: Option<ContractHeader>,
    pub hunks: Vec<AttributedHunk>,
}

/// The diff pane's fully shaped content for the current selection —
/// what `crate::ui::draw_diff_pane` renders, computed once by
/// `crate::run_app` and handed in rather than recomputed per draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffPaneContent {
    /// Nothing to show: no row selected, a directory row (no diff-specific
    /// content of its own — `App::selected_diff_target`'s own doc comment),
    /// or (defensively) a mismatch between `report` and the diff text.
    Empty,
    /// A single symbol's hunks, clipped to its own line range — unchanged
    /// scoping from before this ADR, just wrapped in the same
    /// `DiffPaneContent` shape a file selection now also produces.
    Symbol(DiffSection),
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
    let (sections, show_section_headers): (Vec<&DiffSection>, bool) = match content {
        DiffPaneContent::Empty => return Vec::new(),
        DiffPaneContent::Symbol(section) => (vec![section], false),
        DiffPaneContent::File(sections) => (sections.iter().collect(), true),
    };

    let mut starts = Vec::new();
    let mut line = 0usize;
    for (section_index, section) in sections.iter().enumerate() {
        if section_index > 0 {
            line += 1; // blank line between sections
        }
        if show_section_headers {
            line += 1; // section title line
        }
        if section.contract_header.is_some() {
            line += 2; // "- previous" / "+ current" lines
        }

        for (hunk_index, attributed) in section.hunks.iter().enumerate() {
            if hunk_index > 0 || show_section_headers || section.contract_header.is_some() {
                line += 1; // blank line before this hunk's own header
            }
            starts.push(line);
            line += 1; // the hunk header line itself
            line += attributed.hunk.lines.len();
        }
    }
    starts
}

/// Builds the diff pane's shaped content for `target` (`None` mirrors
/// `App::selected_diff_target` returning `None` — nothing selected, or a
/// directory row). `diff_files` is the whole diff already parsed once by
/// `crate::run_app` (`crate::diff_view::parse_diff_hunks`), not re-parsed
/// here.
pub fn build_diff_pane_content(
    report: &Report,
    diff_files: &[FileHunks],
    target: Option<&DiffTarget>,
) -> DiffPaneContent {
    match target {
        None => DiffPaneContent::Empty,
        Some(DiffTarget::Symbol {
            path,
            range_start,
            range_end,
        }) => build_symbol_content(report, diff_files, path, *range_start, *range_end),
        Some(DiffTarget::File { path }) => build_file_content(report, diff_files, path),
    }
}

fn build_symbol_content(
    report: &Report,
    diff_files: &[FileHunks],
    path: &str,
    range_start: usize,
    range_end: usize,
) -> DiffPaneContent {
    let Some(file_hunks) = file_hunks(diff_files, path) else {
        return DiffPaneContent::Empty;
    };
    let hunks: Vec<AttributedHunk> = file_hunks
        .hunks
        .iter()
        .enumerate()
        .filter(|(_, hunk)| crate::diff_view::hunk_intersects(hunk, range_start, range_end))
        .map(|(source_index, hunk)| AttributedHunk {
            source_index,
            hunk: hunk.clone(),
        })
        .collect();
    if hunks.is_empty() {
        return DiffPaneContent::Empty;
    }

    let symbol = report
        .files
        .iter()
        .find(|file| file.path == path)
        .and_then(|file| {
            file.symbols
                .iter()
                .find(|s| s.range.start == range_start && s.range.end == range_end)
        });
    let title = symbol.map(|s| s.signature.clone()).unwrap_or_default();
    let contract_header = symbol.and_then(contract_header_for_symbol);

    DiffPaneContent::Symbol(DiffSection {
        title,
        contract_header,
        hunks,
    })
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
            contract_header: contract_header_for_symbol(symbol),
            hunks: Vec::new(),
        })
        .collect();
    let mut module_level_hunks: Vec<AttributedHunk> = Vec::new();

    for (source_index, hunk) in file_hunks.hunks.iter().enumerate() {
        // First (source-order) symbol whose range intersects this hunk —
        // ADR 0020's "Alternatives" section rejected attributing a hunk to
        // every overlapping symbol (would misreport total change size for
        // exactly the summary view meant to convey it); first-match keeps
        // every hunk under exactly one section.
        let owner = symbols.iter().position(|symbol| {
            crate::diff_view::hunk_intersects(hunk, symbol.range.start, symbol.range.end)
        });
        let attributed = AttributedHunk {
            source_index,
            hunk: hunk.clone(),
        };
        match owner {
            Some(index) => sections[index].hunks.push(attributed),
            None => module_level_hunks.push(attributed),
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
    fn should_return_empty_when_symbol_file_has_no_matching_diff_hunks() {
        let report = Report {
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", LineRange { start: 1, end: 1 })],
            }],
            ..empty_report()
        };
        let target = DiffTarget::Symbol {
            path: "lib.rs".to_string(),
            range_start: 1,
            range_end: 1,
        };

        let actual = build_diff_pane_content(&report, &[], Some(&target));

        assert_eq!(DiffPaneContent::Empty, actual);
    }

    #[test]
    fn should_clip_symbol_selection_to_its_own_hunks() {
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
        let target = DiffTarget::Symbol {
            path: "lib.rs".to_string(),
            range_start: 1,
            range_end: 2,
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::Symbol(DiffSection {
            title: "fn foo()".to_string(),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            )],
        });
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_include_pure_deletion_hunk_in_symbol_selection_when_it_intersects_the_symbol() {
        // Finding-2 regression: a pure-deletion hunk (`new_range` a
        // zero-width position, `crate::diff_view::Hunk`'s own doc comment)
        // used to always report `hunk_intersects == false`, so it silently
        // vanished from a symbol-scoped diff view entirely instead of
        // showing the deleted lines under the symbol they were removed
        // from.
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
        let target = DiffTarget::Symbol {
            path: "lib.rs".to_string(),
            range_start: 1,
            range_end: 4,
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::Symbol(DiffSection {
            title: "fn foo()".to_string(),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk(
                    "@@ -4 +3,0 @@",
                    Some((3, 2)),
                    vec!["println!(\"removed\");"],
                ),
            )],
        });
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_include_contract_header_when_symbol_selection_signature_changed() {
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
        let target = DiffTarget::Symbol {
            path: "lib.rs".to_string(),
            range_start: 1,
            range_end: 2,
        };

        let actual = build_diff_pane_content(&report, &diff_files, Some(&target));

        let expected = DiffPaneContent::Symbol(DiffSection {
            title: "fn foo(a: i32, b: i32)".to_string(),
            contract_header: Some(ContractHeader {
                previous_signature: "fn foo(a: i32)".to_string(),
                signature: "fn foo(a: i32, b: i32)".to_string(),
            }),
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
            )],
        });
        assert_eq!(expected, actual);
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
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
                )],
            },
            DiffSection {
                title: "fn bar()".to_string(),
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
                contract_header: None,
                hunks: vec![attributed(
                    1,
                    hunk("@@ -10,1 +10,2 @@", Some((10, 11)), vec!["fn foo() {}"]),
                )],
            },
            DiffSection {
                title: MODULE_LEVEL_TITLE.to_string(),
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
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
            )],
        }]);
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_attribute_overlapping_hunk_to_first_symbol_in_source_order() {
        // Two symbols with adjacent, overlapping ranges (a pathological
        // input a real extractor would not normally produce, but the
        // shaping function's contract must still resolve deterministically
        // rather than duplicate the hunk into both sections — ADR 0020's
        // "Alternatives" section rejected duplication as misleading about
        // total change size).
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

        let expected = DiffPaneContent::File(vec![DiffSection {
            title: "fn foo()".to_string(),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,5 @@", Some((3, 4)), vec!["shared line"]),
            )],
        }]);
        assert_eq!(expected, actual);
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
    fn should_start_first_hunk_at_line_zero_for_a_single_section_symbol_selection_with_no_contract_header()
     {
        // No section header shown for a symbol selection (`diff_pane_lines`'s
        // own `show_section_headers` rule) and no contract header here, so
        // the hunk's own header line is the very first line.
        let content = DiffPaneContent::Symbol(DiffSection {
            title: "fn foo()".to_string(),
            contract_header: None,
            hunks: vec![attributed(
                0,
                hunk(
                    "@@ -1,1 +1,2 @@",
                    Some((1, 2)),
                    vec!["fn a() {}", "fn foo() {}"],
                ),
            )],
        });

        let actual = hunk_start_lines(&content);

        assert_eq!(vec![0], actual);
    }

    #[test]
    fn should_offset_hunk_start_by_contract_header_lines_when_symbol_selection_has_one() {
        // 2 contract-header lines precede the hunk's own header line.
        let content = DiffPaneContent::Symbol(DiffSection {
            title: "fn foo(a, b)".to_string(),
            contract_header: Some(ContractHeader {
                previous_signature: "fn foo(a)".to_string(),
                signature: "fn foo(a, b)".to_string(),
            }),
            hunks: vec![attributed(
                0,
                hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo(a, b) {}"]),
            )],
        });

        let actual = hunk_start_lines(&content);

        assert_eq!(vec![3], actual);
    }

    #[test]
    fn should_offset_second_hunk_start_by_first_hunk_header_and_body_length() {
        // First hunk: header (1 line) + 2 body lines = 3 lines, then 1 blank
        // separator line before the second hunk's own header.
        let content = DiffPaneContent::Symbol(DiffSection {
            title: "fn foo()".to_string(),
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
        });

        let actual = hunk_start_lines(&content);

        assert_eq!(vec![0, 4], actual);
    }

    #[test]
    fn should_offset_hunk_start_by_section_header_and_separator_lines_for_a_file_selection() {
        // Section 0 (file selection: `show_section_headers` is always true,
        // so the blank-before-hunk rule fires even for the section's first
        // hunk): title(0), blank(1), header(2), 1 body line(3) — hunk starts
        // at line 2. Section 1: blank separator between sections(4),
        // title(5), blank before its hunk(6), header(7) — hunk starts at 7.
        let content = DiffPaneContent::File(vec![
            DiffSection {
                title: "fn foo()".to_string(),
                contract_header: None,
                hunks: vec![attributed(
                    0,
                    hunk("@@ -1,1 +1,2 @@", Some((1, 2)), vec!["fn foo() {}"]),
                )],
            },
            DiffSection {
                title: "fn bar()".to_string(),
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
}
