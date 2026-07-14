//! Directory tree view-model (ADR 0015): the TUI's entry view is the
//! directory tree of changed files, not the call-graph tree — nesting
//! depth conveys architecture, and each row carries aggregate badges.
//!
//! [`build_tree`] is a pure function from [`Report`] alone: same `Report`
//! in, same [`Tree`] out, no IO, no ordering decisions (ordering is a
//! separate concern, see `crate::order`).

use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::file_size::FileSizeBand;
use rinkaku_core::render::{Report, SkipReason};
use std::collections::{BTreeMap, HashMap};

mod tests_section;

/// A symbol's identity, as carried by a [`NodeKind::Symbol`] leaf — enough
/// for the entry view to render a badge-worthy row and for the detail view
/// (`crate::detail`) to look the full symbol back up in the `Report` it was
/// built from, without this crate duplicating `ExtractedSymbol`'s full
/// shape (signature, dependencies, ...) into the view-model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRef {
    /// Matches [`rinkaku_core::graph::Node::id`] for a present symbol, or is
    /// synthesized as `{path}::{name}` for a [`RemovedSymbol`] (which has no
    /// stable id of its own — see `RemovedSymbol`'s doc comment in
    /// `rinkaku-core`). Not guaranteed unique for two removed symbols
    /// sharing `(path, name)`, same limitation `render.rs`'s Markdown
    /// rendering already accepts for removed symbols.
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    /// `None` when this symbol is a [`RemovedSymbol`] — a removed symbol
    /// was never classified against itself (there is no head-side symbol to
    /// classify), only reported as `Report.removed` because a base-side
    /// match went missing entirely.
    pub classification: Option<Classification>,
    pub removed: bool,
    /// Mirrors [`rinkaku_core::extract::ExtractedSymbol::is_test`] (ADR
    /// 0035): `true` for a test symbol that survives into the production
    /// tree because it lives in a *mixed* file alongside non-test symbols
    /// (a whole-test-file's symbols never reach the production tree at
    /// all — see `TreeNode::test_symbol_count`'s doc comment — so this
    /// field only ever matters for the mixed case). `false` for a
    /// [`RemovedSymbol`], which carries no `is_test` flag of its own —
    /// there is no head-side AST context left to classify a removed
    /// symbol's test-ness by.
    pub is_test: bool,
}

/// What kind of thing a [`TreeNode`] represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    /// A directory. May be a collapsed chain of several path segments (see
    /// `build_tree`'s doc comment on collapsing) — `name` is the full
    /// collapsed label (e.g. `"a/b/c"`), not just the last segment.
    Dir,
    /// A changed file. `name` is the file's base name; the file's full path
    /// is reconstructed by joining ancestor `Dir`/`File` labels, which the
    /// tree itself does not do — callers needing the full path should track
    /// it during traversal (kept simple here since this stage has no
    /// renderer yet to demand it).
    File,
    /// A leaf: one changed or removed symbol.
    Symbol(SymbolRef),
    /// A synthetic grouping node, not derived from any single file/symbol
    /// (ADR 0035 Phase B) — see [`tests_section`] for construction and
    /// [`crate::order`] for how it sorts. A distinct variant rather than
    /// a `Dir` with a synthetic path, since `crate::order` looks up a
    /// `Dir`'s rank by path, and a fake path there risks an accidental
    /// rank/collision bug (ADR 0035's Alternatives).
    Section(SectionKind),
    /// A synthetic grouping of a *mixed* file's test symbols (visual-
    /// encoding prototype), nested as the last child of that `File` node.
    /// Unlike `Section` (which pulls whole test files out to a top-level
    /// trailing group), this groups the *subset* of one file's symbols
    /// that are test code, so a file mixing production and test code
    /// (e.g. `mermaid.rs` with a dozen `#[cfg(test)]` functions) collapses
    /// to one `N tests` row instead of flooding the tree with individual
    /// `test`-badged rows. `count` is the number of test symbols grouped
    /// (equal to `children.len()`, kept as its own field so `row_view` can
    /// render the `N tests` label without counting children first).
    TestGroup { count: usize },
}

/// Which kind of synthetic grouping a [`NodeKind::Section`] is. An enum
/// rather than a unit variant so a second section kind, if ever needed,
/// does not require another `NodeKind` variant (and therefore another
/// wave of exhaustive-match updates across `nav`/`order`/`row_view`/
/// `detail`/`app`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Tests,
}

/// The synthetic path a [`NodeKind::Section`] node carries on
/// [`TreeNode::path`] — doubles as `crate::nav::Nav`'s collapse-state key
/// (which is generic over `TreeNode::path`, so a section needs *some*
/// stable path to participate) and the row label. `__tests__` cannot
/// collide with a real slash-joined file path.
pub const TESTS_SECTION_PATH: &str = "__tests__";

impl SectionKind {
    pub fn label(self) -> &'static str {
        match self {
            SectionKind::Tests => "Tests",
        }
    }
}

/// Badges aggregated bottom-up for a [`TreeNode`] (ADR 0015/0016): every
/// count also includes this node's descendants, so a directory's badge
/// summarizes everything nested under it without a reader needing to
/// expand it first.
///
/// Field semantics, decided here since ADR 0015/0016 left them open:
/// - `changed_symbols`: count of present (non-removed) symbols, i.e. every
///   [`SymbolRef`] with `removed == false`. Removed symbols are *not*
///   counted here — a removed symbol has no signature/graph presence of
///   its own, so folding it into "changed" would blur "this many symbols
///   still exist and changed" with "this many disappeared".
/// - `contract_changes`: count of symbols whose classification is
///   [`Classification::SignatureChanged`], **plus** every removed symbol.
///   Removal is unambiguously a contract change — the API surface the
///   removed symbol represented is gone — so it counts here even though it
///   is excluded from `changed_symbols` above.
/// - `fan_in`: **sum** (not max) of `used_by.len()` for every high-fan-in
///   symbol contained in this node's subtree. Sum was chosen over max
///   because a directory containing several independently risky high-
///   fan-in symbols should read as riskier than one containing a single
///   such symbol with the same peak fan-in — max would hide that
///   difference.
///
///   This badge's `fan_in` is deliberately **not** the same computation
///   `crate::detail::build_detail` uses for a single symbol's `used_by`:
///   this badge only counts a symbol at all once it clears `FanIn`'s own
///   fan-in >= 2 threshold (see `symbol_badges`'s doc comment), while the
///   detail pane's `used_by` reads `report.graph.edges` directly and so
///   also surfaces a fan-in of 0 or 1. A symbol with exactly one referrer
///   therefore shows up in its own detail view's `used_by` but contributes
///   nothing to any ancestor directory's `fan_in` badge here — expected,
///   not a bug, since the badge's whole purpose is to flag high-fan-in
///   symbols specifically, not fan-in in general.
/// - `own_file_size_band`/`own_file_line_count`: this file node's own
///   [`FileSizeBand`] and line count (ADR 0028 amendment), `None` for
///   every non-file node. Deliberately **not** merged upward — a
///   directory has no single band of its own, only the aggregates below.
/// - `file_size_warn_count`/`file_size_split_count`: bottom-up count of
///   file nodes in this subtree at [`FileSizeBand::Warn`]/`Split`
///   respectively, rendered as `warn:N`/`split:N` on directory rows.
///   `Normal`/`Watch` files contribute to neither — those two bands are
///   shown per-file only, not aggregated, since they are not
///   "attention-worthy" the way `Warn`/`Split` are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Badges {
    pub changed_symbols: usize,
    pub contract_changes: usize,
    pub fan_in: usize,
    pub own_file_size_band: Option<FileSizeBand>,
    pub own_file_line_count: Option<usize>,
    pub file_size_warn_count: usize,
    pub file_size_split_count: usize,
}

impl Badges {
    fn merge(&mut self, other: Badges) {
        self.changed_symbols += other.changed_symbols;
        self.contract_changes += other.contract_changes;
        self.fan_in += other.fan_in;
        self.file_size_warn_count += other.file_size_warn_count;
        self.file_size_split_count += other.file_size_split_count;
    }
}

/// One node in the [`Tree`]: a directory, file, or symbol, with its
/// bottom-up aggregated [`Badges`] and its children in source order (before
/// any topological/A-Z reordering — see `crate::order`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeNode {
    pub kind: NodeKind,
    /// Full slash-joined path from the tree root to this node, used as a
    /// stable key by `crate::nav`'s collapse-state map. For a `Dir` this is
    /// the collapsed chain (e.g. `"a/b/c"`); for a `File`/`Symbol` it is the
    /// file's path (a `Symbol`'s path is its containing file's path, not a
    /// path-plus-symbol-name compound, since a symbol's [`SymbolRef::id`]
    /// already disambiguates it within that file).
    pub path: String,
    pub badges: Badges,
    pub children: Vec<TreeNode>,
    /// `Some` only for a [`NodeKind::File`] node built from
    /// `report.skipped` (a file rinkaku could not extract symbols from —
    /// see `SkipReason`), `None` for every other node including an
    /// ordinary analyzed `File`. Kept as a field on `TreeNode` rather than a
    /// new `NodeKind` variant so `crate::app`/`crate::order`'s existing
    /// exhaustive `match`es over `NodeKind` (dispatching detail/diff/blast-
    /// radius panes and sibling ordering) keep treating a skipped file exactly
    /// like any other file row — it already has the right shape (a
    /// childless file with a path), it just additionally carries *why*
    /// rinkaku skipped it, for `row_view`/the detail pane to surface.
    pub skip_reason: Option<SkipReason>,
    /// `Some(symbol_count)` for a [`NodeKind::File`] node built from an
    /// entry in `report.tests` (ADR 0009): either a file whose changed
    /// symbols were *all* test code (no `FileReport` in `report.files` at
    /// all for it, see `pipeline::partition_test_symbols`'s doc comment —
    /// without this it would be invisible in the tree the same way a
    /// skipped file is), or a *mixed* file that has both real (non-test)
    /// symbols in `report.files` and a nonzero test-symbol count in
    /// `report.tests` for the same path — `partition_test_symbols`
    /// populates both independently, it is not an either/or split, so
    /// `symbols` and `test_symbol_count` are **not** mutually exclusive
    /// here (unlike `skip_reason`, which is: a skipped file by construction
    /// has no `FileReport` entry at all). `None` when the file has no
    /// `report.tests` entry, i.e. every changed symbol in it (if any) is
    /// non-test.
    pub test_symbol_count: Option<usize>,
}

/// The whole directory tree built from one [`Report`]. `roots` holds the
/// top-level entries (in source order); there is no single synthetic root
/// node, mirroring how a file explorer shows multiple top-level
/// directories/files rather than one root labeled `"."`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tree {
    pub roots: Vec<TreeNode>,
}

/// Builds the directory tree over every file with content in `report`
/// (`report.files`, including files with an empty `symbols` list — e.g. a
/// pure rename, still shown as a `File` node with zero badges — and
/// `report.removed`'s files, which may not otherwise appear in `files` at
/// all if every symbol in that file was removed), plus every whole-test
/// file summarized in `report.tests` and every non-`Generated` entry in
/// `report.skipped` (a file rinkaku could not extract symbols from at
/// all) — both of which otherwise have no `TreeNode` of their own and so
/// were previously invisible in the tree entirely (see `TreeNode`'s
/// `skip_reason`/`test_symbol_count` doc comments).
///
/// `SkipReason::Generated` entries are dropped from the tree the same way
/// `render_markdown` drops them from "Skipped files" — a `.gitattributes`
/// declaration or linguist-compatible marker has already told the
/// repository this file is uninteresting to diff-review (ADR 0010/0011),
/// so surfacing it in the TUI would just be noise a reviewer has to
/// scroll past, same reasoning as the Markdown renderer's own comment.
///
/// Construction is a pure function of `report` alone and deterministic:
/// files are visited in `report.files` order (already source order per
/// `pipeline::analyze_diff`), then `report.removed`, then `report.tests`,
/// then `report.skipped`, for any additional files/symbols not already
/// covered, and directory chains are inserted in that same discovery
/// order. A path present in more than one of these sources merges into one
/// `TreeNode` the same way `insert_file`/`insert_removed` already merge on a
/// shared path, rather than producing a duplicate row. `files`/`tests`
/// overlapping on a path is a real, expected case — `pipeline::partition_test_symbols`
/// can emit both a `FileReport` and a `TestFileSummary` for one mixed file
/// (`TreeNode::test_symbol_count`'s own doc comment) — but `skipped`
/// overlapping either `files` or `tests` on the same path is not expected
/// from `pipeline::analyze_diff`'s own invariants (a skipped file has no
/// `FileReport`/`TestFileSummary` of its own), and is only debug-asserted
/// against, not handled gracefully, by `insert_skipped`/`insert_test_file`.
///
/// **Single-child directory collapsing**: a directory whose only content is
/// exactly one child directory (and nothing else — no files or symbols of
/// its own) collapses with that child into one `Dir` node labeled with the
/// full joined path (e.g. `"src/foo/bar"` instead of three nested `"src"` /
/// `"foo"` / `"bar"` nodes). This is what reviewers expect from familiar
/// file-tree UIs (VS Code's explorer, `git log --stat` style tools): a
/// three-deep chain that exists only to reach one file underneath carries
/// no architectural signal on its own, so collapsing it removes a click/
/// scroll without losing information — the full path is still shown, just
/// on one row. Collapsing stops as soon as a directory has more than one
/// child, or has files/symbols of its own alongside a subdirectory.
pub fn build_tree(report: &Report) -> Tree {
    let fan_in_by_id: HashMap<&str, usize> = report
        .fan_ins
        .iter()
        .map(|fan_in| (fan_in.id.as_str(), fan_in.used_by.len()))
        .collect();

    // ADR 0028 amendment: built from `file_size_bands` (every analyzed
    // file), not `file_size_warnings` (the Warn/Split subset) — so a
    // file row can always show its line count, not only when it crosses
    // the warn threshold.
    let file_size_by_path: HashMap<&str, (FileSizeBand, usize)> = report
        .file_size_bands
        .iter()
        .map(|entry| (entry.path.as_str(), (entry.band, entry.line_count)))
        .collect();

    let mut production = TreeBuilder::new(fan_in_by_id.clone(), file_size_by_path.clone(), true);
    let mut tests = TreeBuilder::new(fan_in_by_id, file_size_by_path, false);

    // A mixed file (some non-test symbols alongside some test symbols)
    // always stays in `production` untouched — only a *whole* test file
    // routes into `tests` (ADR 0035 Phase B, see
    // `tests_section::is_whole_test_file`).
    for file in &report.files {
        if tests_section::is_whole_test_file(&file.path, &file.symbols) {
            tests.insert_file(&file.path, &file.symbols);
        } else {
            production.insert_file(&file.path, &file.symbols);
        }
    }
    // A `RemovedSymbol` carries no `is_test` flag and is never checked
    // against `is_test_path` here, so it always stays in production
    // regardless of the rest of its file's fate.
    for removed in &report.removed {
        production.insert_removed(&removed.path, removed);
    }
    for test_file in &report.tests {
        production.insert_test_file(&test_file.path, test_file.symbol_count);
    }
    for skipped in &report.skipped {
        if !matches!(skipped.reason, SkipReason::Generated) {
            production.insert_skipped(&skipped.path, skipped.reason);
        }
    }

    let mut roots = production.finish().roots;
    if let Some(section) = tests_section::wrap_section(tests.finish().roots) {
        roots.push(section);
    }

    Tree { roots }
}

/// Intermediate mutable tree used only during construction — a
/// [`BTreeMap`]-backed trie keyed by path segment, so repeated
/// `insert_file`/`insert_removed` calls sharing a path prefix merge into
/// the same directory node instead of creating duplicates. Converted into
/// the immutable [`Tree`] (with badges aggregated and collapsing applied)
/// by [`TreeBuilder::finish`].
struct TreeBuilder<'a> {
    root: DirBuilder,
    /// `report.fan_ins`, keyed by [`rinkaku_core::graph::NodeId`], so a
    /// symbol's fan-in badge can be looked up by id while walking
    /// `report.files` — built once in `build_tree` rather than per-symbol,
    /// since `report.fan_ins` doesn't change during one `build_tree` call.
    fan_in_by_id: HashMap<&'a str, usize>,
    /// `report.file_size_bands` keyed by path (ADR 0028 amendment):
    /// covers every analyzed file, not only `Warn`/`Split` as the
    /// pre-amendment `file_size_warnings`-derived map did.
    file_size_by_path: HashMap<&'a str, (FileSizeBand, usize)>,
    /// Whether a mixed file's test symbols fold into a trailing
    /// `TestGroup` child (visual-encoding prototype): `true` for the
    /// `production` builder, `false` for the `tests` builder — a
    /// whole-test file already reads as test code via its enclosing
    /// `Section::Tests` (ADR 0035 Phase B), so grouping its symbols again
    /// one level deeper would be redundant nesting with nothing left to
    /// distinguish from production siblings (it has none).
    group_test_symbols: bool,
}

#[derive(Default)]
struct DirBuilder {
    // BTreeMap only to get a deterministic iteration order out of the
    // builder itself as a safety net; `finish` overrides visit order with
    // each node's recorded `insertion_order` so actual output order still
    // matches source order, not alphabetical.
    dirs: BTreeMap<String, DirBuilder>,
    files: BTreeMap<String, FileBuilder>,
    insertion_order: Vec<String>,
}

#[derive(Default)]
struct FileBuilder {
    /// Each symbol alongside its new-side `ExtractedSymbol::range.start`,
    /// kept only long enough for `build_file_node` to place a `TestGroup`
    /// child at its source position (ADR 0045) — `SymbolRef` itself carries
    /// no line number.
    symbols: Vec<(SymbolRef, usize)>,
    /// Set by `insert_skipped` — see `TreeNode::skip_reason`'s doc comment.
    skip_reason: Option<SkipReason>,
    /// Set by `insert_test_file` — see `TreeNode::test_symbol_count`'s doc
    /// comment.
    test_symbol_count: Option<usize>,
}

impl<'a> TreeBuilder<'a> {
    fn new(
        fan_in_by_id: HashMap<&'a str, usize>,
        file_size_by_path: HashMap<&'a str, (FileSizeBand, usize)>,
        group_test_symbols: bool,
    ) -> Self {
        Self {
            root: DirBuilder::default(),
            fan_in_by_id,
            file_size_by_path,
            group_test_symbols,
        }
    }

    fn insert_file(&mut self, path: &str, symbols: &[rinkaku_core::extract::ExtractedSymbol]) {
        let segments: Vec<&str> = path.split('/').collect();
        let file_builder = self.root.file_at(&segments);
        // `report.files` and `report.skipped` never overlap on a path — a
        // skipped file by construction has no `FileReport` at all
        // (`pipeline::analyze_diff`'s invariant). `report.files` and
        // `report.tests` *can* legitimately overlap, though: a mixed file
        // (some real symbols, some `#[cfg(test)]`-style test symbols) gets
        // both a `FileReport` (its non-test symbols) and a `TestFileSummary`
        // (its test count) for the same path —
        // `pipeline::partition_test_symbols`'s doc comment. So only the
        // skip_reason half of the old combined check is a real invariant;
        // asserting against `test_symbol_count` here would reject a
        // reachable, correct `Report` (this crate's own dogfood run against
        // this repo hit exactly that: `rinkaku-tui/src/app.rs` has both real
        // and test symbols changed in the same diff). Debug-only since this
        // is still a caller contract violation for the skip_reason case, not
        // a condition `build_tree` itself needs to handle gracefully in
        // release builds.
        debug_assert!(
            file_builder.skip_reason.is_none(),
            "path {path:?} has real symbols but was already listed in report.skipped — \
             report.files/report.skipped must not overlap on the same path"
        );
        for symbol in symbols {
            file_builder.symbols.push((
                SymbolRef {
                    id: symbol.id.clone(),
                    name: symbol.name.clone(),
                    kind: symbol.kind,
                    classification: symbol.classification,
                    removed: false,
                    is_test: symbol.is_test,
                },
                symbol.range.start,
            ));
        }
    }

    fn insert_removed(&mut self, path: &str, removed: &rinkaku_core::extract::RemovedSymbol) {
        let segments: Vec<&str> = path.split('/').collect();
        let file_builder = self.root.file_at(&segments);
        file_builder.symbols.push((
            SymbolRef {
                id: format!("{path}::{}", removed.name),
                name: removed.name.clone(),
                kind: removed.kind,
                classification: None,
                removed: true,
                // `RemovedSymbol` carries no `is_test` flag of its own — see
                // `SymbolRef::is_test`'s doc comment.
                is_test: false,
            },
            // No range to report; harmless since a removed symbol is never
            // `is_test` (`SymbolRef::is_test`'s doc comment), so it never
            // enters the comparison this line feeds (ADR 0045).
            usize::MAX,
        ));
    }

    /// Inserts a whole- or mixed-test-file summary (`report.tests`, see
    /// `TreeNode::test_symbol_count`'s doc comment) into the node for
    /// `path` — a childless `File` node when the file has no `FileReport`
    /// at all (all its changed symbols were test code), or an existing node
    /// already carrying real `symbols` from a prior `insert_file` call when
    /// the file is mixed. Either way there is no per-symbol data for the
    /// *test* half to nest under, only the count
    /// `pipeline::partition_test_symbols` kept.
    fn insert_test_file(&mut self, path: &str, symbol_count: usize) {
        let segments: Vec<&str> = path.split('/').collect();
        let file_builder = self.root.file_at(&segments);
        // Only guard the genuinely-invariant case: `report.skipped` never
        // overlaps `report.tests` (a skipped file has no symbols of any
        // kind to partition into test/non-test in the first place). Real
        // symbols being present here is expected for a mixed file —
        // `insert_file`'s own doc comment on why that is not a contract
        // violation.
        debug_assert!(
            file_builder.skip_reason.is_none(),
            "path {path:?} was already listed in report.skipped but was also summarized in \
             report.tests — report.skipped/report.tests must not overlap on the same path"
        );
        file_builder.test_symbol_count = Some(symbol_count);
    }

    /// Inserts a skipped-file entry (`report.skipped`, see
    /// `TreeNode::skip_reason`'s doc comment) as a childless `File` node.
    fn insert_skipped(&mut self, path: &str, reason: SkipReason) {
        let segments: Vec<&str> = path.split('/').collect();
        let file_builder = self.root.file_at(&segments);
        // Same overlap contract as `insert_file`'s debug_assert: a skipped
        // file has no `FileReport`/`TestFileSummary` entry of its own, so
        // neither real symbols nor a test count should already be set here.
        debug_assert!(
            file_builder.symbols.is_empty() && file_builder.test_symbol_count.is_none(),
            "path {path:?} was already listed in report.files/report.tests but was also listed \
             in report.skipped — report.skipped must not overlap report.files/report.tests on \
             the same path"
        );
        file_builder.skip_reason = Some(reason);
    }

    fn finish(self) -> Tree {
        Tree {
            roots: self.root.into_nodes(
                String::new(),
                &self.fan_in_by_id,
                &self.file_size_by_path,
                self.group_test_symbols,
            ),
        }
    }
}

impl DirBuilder {
    /// Descends (creating as needed) to the directory containing the last
    /// path segment, returning the [`FileBuilder`] for that segment —
    /// shared by both `insert_file` and `insert_removed` so a file touched
    /// by both a present symbol and a removed one lands in the same node.
    fn file_at(&mut self, segments: &[&str]) -> &mut FileBuilder {
        match segments {
            [] => unreachable!("split('/') on a non-empty path always yields at least one segment"),
            [file_name] => {
                if !self.files.contains_key(*file_name) {
                    self.insertion_order.push(format!("f:{file_name}"));
                    self.files
                        .insert(file_name.to_string(), FileBuilder::default());
                }
                self.files.get_mut(*file_name).expect("just inserted")
            }
            [dir_name, rest @ ..] => {
                if !self.dirs.contains_key(*dir_name) {
                    self.insertion_order.push(format!("d:{dir_name}"));
                    self.dirs
                        .insert(dir_name.to_string(), DirBuilder::default());
                }
                self.dirs
                    .get_mut(*dir_name)
                    .expect("just inserted")
                    .file_at(rest)
            }
        }
    }

    /// Converts this builder into `TreeNode`s in discovery (`insertion_order`)
    /// order, applying single-child directory collapsing (see
    /// `build_tree`'s doc comment) and computing bottom-up [`Badges`] as it
    /// goes. `fan_in_by_id` is threaded through to leaf symbols unchanged —
    /// see `symbol_badges`.
    fn into_nodes(
        self,
        prefix: String,
        fan_in_by_id: &HashMap<&str, usize>,
        file_size_by_path: &HashMap<&str, (FileSizeBand, usize)>,
        group_test_symbols: bool,
    ) -> Vec<TreeNode> {
        let DirBuilder {
            mut dirs,
            mut files,
            insertion_order,
        } = self;

        let mut nodes = Vec::with_capacity(insertion_order.len());
        for key in insertion_order {
            if let Some(dir_name) = key.strip_prefix("d:") {
                let child = dirs.remove(dir_name).expect("recorded in insertion_order");
                nodes.push(build_dir_node(
                    dir_name.to_string(),
                    &prefix,
                    child,
                    fan_in_by_id,
                    file_size_by_path,
                    group_test_symbols,
                ));
            } else if let Some(file_name) = key.strip_prefix("f:") {
                let file = files
                    .remove(file_name)
                    .expect("recorded in insertion_order");
                nodes.push(build_file_node(
                    file_name.to_string(),
                    &prefix,
                    file,
                    fan_in_by_id,
                    file_size_by_path,
                    group_test_symbols,
                ));
            }
        }
        nodes
    }
}

/// Builds one directory's [`TreeNode`], collapsing single-child directory
/// chains into this node rather than nesting them (see `build_tree`'s doc
/// comment). Collapsing is applied repeatedly: after folding in one child
/// directory, the result might itself now be foldable again if that
/// child's own single child was also a lone directory — the `loop` below
/// keeps folding until the node has more than one child or a non-directory
/// child of its own.
fn build_dir_node(
    name: String,
    prefix: &str,
    mut dir: DirBuilder,
    fan_in_by_id: &HashMap<&str, usize>,
    file_size_by_path: &HashMap<&str, (FileSizeBand, usize)>,
    group_test_symbols: bool,
) -> TreeNode {
    let mut label = name;
    loop {
        let only_child_is_lone_dir =
            dir.files.is_empty() && dir.dirs.len() == 1 && dir.insertion_order.len() == 1;
        if !only_child_is_lone_dir {
            break;
        }
        let (child_name, child_dir) = dir
            .dirs
            .into_iter()
            .next()
            .expect("dirs.len() == 1 just checked");
        label = format!("{label}/{child_name}");
        dir = child_dir;
    }

    let path = join_path(prefix, &label);
    let children = dir.into_nodes(
        path.clone(),
        fan_in_by_id,
        file_size_by_path,
        group_test_symbols,
    );
    let mut badges = Badges::default();
    for child in &children {
        badges.merge(child.badges);
    }

    TreeNode {
        kind: NodeKind::Dir,
        path,
        badges,
        children,
        skip_reason: None,
        test_symbol_count: None,
    }
}

fn build_file_node(
    name: String,
    prefix: &str,
    file: FileBuilder,
    fan_in_by_id: &HashMap<&str, usize>,
    file_size_by_path: &HashMap<&str, (FileSizeBand, usize)>,
    group_test_symbols: bool,
) -> TreeNode {
    let path = join_path(prefix, &name);
    let (own_file_size_band, own_file_line_count) = match file_size_by_path.get(path.as_str()) {
        Some((band, line_count)) => (Some(*band), Some(*line_count)),
        None => (None, None),
    };
    // A Warn/Split file contributes 1 to the matching aggregate count so
    // an ancestor directory's `Badges::merge` sums the correct per-band
    // totals — see `Badges`' doc comment.
    let file_size_warn_count = usize::from(matches!(own_file_size_band, Some(FileSizeBand::Warn)));
    let file_size_split_count =
        usize::from(matches!(own_file_size_band, Some(FileSizeBand::Split)));
    let mut badges = Badges {
        own_file_size_band,
        own_file_line_count,
        file_size_warn_count,
        file_size_split_count,
        ..Badges::default()
    };
    // Grouping only ever applies to the `production` builder — a
    // whole-test file (routed entirely into the `tests` builder, see
    // `build_tree`) already reads as test code via its enclosing
    // `Section::Tests`, so its symbols stay flat, ungrouped children here
    // (`TreeBuilder::group_test_symbols`'s doc comment).
    let (test_symbols, production_symbols): (Vec<_>, Vec<_>) = if group_test_symbols {
        file.symbols.into_iter().partition(|(s, _)| s.is_test)
    } else {
        (Vec::new(), file.symbols)
    };
    // ADR 0045: the group's position among its production siblings is
    // decided before either list loses its line numbers below.
    let test_group_insert_at = test_group_insert_index(&test_symbols, &production_symbols);

    let mut children: Vec<TreeNode> = production_symbols
        .into_iter()
        .map(|(symbol_ref, _)| {
            let symbol_badges = symbol_badges(&symbol_ref, fan_in_by_id);
            badges.merge(symbol_badges);
            TreeNode {
                kind: NodeKind::Symbol(symbol_ref),
                path: path.clone(),
                badges: symbol_badges,
                children: Vec::new(),
                skip_reason: None,
                test_symbol_count: None,
            }
        })
        .collect();

    let test_symbols: Vec<SymbolRef> = test_symbols.into_iter().map(|(s, _)| s).collect();
    if let Some(test_group) = build_test_group_node(&path, test_symbols, fan_in_by_id) {
        badges.merge(test_group.badges);
        children.insert(test_group_insert_at, test_group);
    }

    TreeNode {
        kind: NodeKind::File,
        path,
        badges,
        children,
        skip_reason: file.skip_reason,
        test_symbol_count: file.test_symbol_count,
    }
}

/// Where a mixed file's `TestGroup` child belongs among `production_symbols`
/// (ADR 0045): immediately before the first production symbol whose line
/// starts after the earliest test symbol's line, or after every production
/// symbol (`production_symbols.len()`) when none does — the common "trailing
/// `#[cfg(test)] mod tests` block" case. `test_symbols` empty returns 0
/// (unused by the caller in that case, since `build_test_group_node` returns
/// `None` for an empty list and no insertion happens at all).
fn test_group_insert_index(
    test_symbols: &[(SymbolRef, usize)],
    production_symbols: &[(SymbolRef, usize)],
) -> usize {
    let Some(earliest_test_line) = test_symbols.iter().map(|(_, line)| *line).min() else {
        return 0;
    };
    production_symbols
        .iter()
        .position(|(_, line)| *line > earliest_test_line)
        .unwrap_or(production_symbols.len())
}

/// A single symbol's own (non-aggregated) badge contribution.
/// `fan_in_by_id` is `report.fan_ins` keyed by id (see `build_tree`): a
/// symbol not present there (fan-in < 2, or a removed symbol — never a
/// graph node) contributes zero fan-in, same as `FanIn`'s own >= 2
/// threshold.
fn symbol_badges(symbol_ref: &SymbolRef, fan_in_by_id: &HashMap<&str, usize>) -> Badges {
    Badges {
        changed_symbols: if symbol_ref.removed { 0 } else { 1 },
        contract_changes: if symbol_ref.removed
            || symbol_ref.classification == Some(Classification::SignatureChanged)
        {
            1
        } else {
            0
        },
        fan_in: fan_in_by_id
            .get(symbol_ref.id.as_str())
            .copied()
            .unwrap_or(0),
        // File size is a per-file attribute (ADR 0028), not a per-symbol
        // one, so a symbol never contributes to any of the file-size
        // fields.
        own_file_size_band: None,
        own_file_line_count: None,
        file_size_warn_count: 0,
        file_size_split_count: 0,
    }
}

/// The synthetic path suffix a mixed file's [`NodeKind::TestGroup`] child
/// carries on [`TreeNode::path`] — appended to the file's own path so the
/// group has a stable, distinct key for `crate::nav::Nav`'s collapse-state
/// map (which is generic over `TreeNode::path`, same rationale as
/// [`TESTS_SECTION_PATH`]). Cannot collide with a real file path since no
/// file path contains `::`.
const TEST_GROUP_PATH_SUFFIX: &str = "::tests";

/// Builds the `TestGroup` child node grouping `test_symbols` under a mixed
/// file, or `None` when there are no test symbols to group (the common
/// case: most files are pure production or pure test, the latter routed
/// entirely into [`tests_section`] instead — see `build_tree`'s doc
/// comment).
fn build_test_group_node(
    file_path: &str,
    test_symbols: Vec<SymbolRef>,
    fan_in_by_id: &HashMap<&str, usize>,
) -> Option<TreeNode> {
    if test_symbols.is_empty() {
        return None;
    }

    let count = test_symbols.len();
    let mut badges = Badges::default();
    let children: Vec<TreeNode> = test_symbols
        .into_iter()
        .map(|symbol_ref| {
            let symbol_badges = symbol_badges(&symbol_ref, fan_in_by_id);
            badges.merge(symbol_badges);
            TreeNode {
                kind: NodeKind::Symbol(symbol_ref),
                path: file_path.to_string(),
                badges: symbol_badges,
                children: Vec::new(),
                skip_reason: None,
                test_symbol_count: None,
            }
        })
        .collect();

    Some(TreeNode {
        kind: NodeKind::TestGroup { count },
        path: format!("{file_path}{TEST_GROUP_PATH_SUFFIX}"),
        badges,
        children,
        skip_reason: None,
        test_symbol_count: None,
    })
}

fn join_path(prefix: &str, segment: &str) -> String {
    if prefix.is_empty() {
        segment.to_string()
    } else {
        format!("{prefix}/{segment}")
    }
}

#[cfg(test)]
#[path = "tree_tests/mod.rs"]
mod tests;
