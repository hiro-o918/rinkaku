//! Directory tree view-model (ADR 0015): the TUI's entry view is the
//! directory tree of changed files, not the call-graph tree — nesting
//! depth conveys architecture, and each row carries aggregate badges.
//!
//! [`build_tree`] is a pure function from [`Report`] alone: same `Report`
//! in, same [`Tree`] out, no IO, no ordering decisions (ordering is a
//! separate concern, see `crate::order`).

use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::file_size::FileSizeSeverity;
use rinkaku_core::render::{Report, SkipReason};
use std::collections::{BTreeMap, HashMap};

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
    /// (ADR 0035 Phase B): currently only [`SectionKind::Tests`], a
    /// trailing node appended to [`Tree::roots`] holding every *whole*
    /// test file, keeping their directory nesting but sorted A-Z
    /// unconditionally rather than participating in
    /// `crate::order`'s topological/alphabetical toggle — there is no
    /// production dependency story left to tell once everything under a
    /// section is test code (see [`rank_directories`](crate::order::rank_directories)'s
    /// own test-exclusion, which is a separate, complementary mechanism
    /// for symbols that stay in the *production* tree). Deliberately a
    /// distinct variant rather than reusing `Dir` with a synthetic path:
    /// a `Dir` node is looked up by path in `crate::order`'s per-path
    /// `DirRank` map, and giving a `Section` a fake path there would risk
    /// an accidental rank/collision bug via a "this magic path never
    /// gets a rank" convention a future edit could silently violate — see
    /// ADR 0035's Alternatives for the full comparison.
    Section(SectionKind),
}

/// Which kind of synthetic grouping a [`NodeKind::Section`] is. A
/// one-member enum today (ADR 0035 only introduces the Tests section),
/// kept as an enum rather than a unit variant so a second section kind
/// (if one is ever needed) does not require another `NodeKind` variant
/// and therefore another wave of exhaustive-match updates across
/// `nav.rs`/`order.rs`/`row_view.rs`/`detail.rs`/`app`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Tests,
}

/// The synthetic path a [`NodeKind::Section`] node carries on its
/// [`TreeNode::path`] — used as the collapse-state key (`crate::nav::Nav`
/// tracks collapse state generically by `TreeNode::path`, with no
/// `NodeKind`-specific branch, so a section needs *some* stable path to
/// participate) and the row label. Chosen to never collide with a real
/// slash-joined file/directory path: no file path can contain two
/// consecutive underscores by itself, but this is intentionally also
/// distinctive rather than merely legal — a reader grepping the source
/// for this literal should immediately see it is synthetic, not a path
/// that happens to exist in some repository.
pub const TESTS_SECTION_PATH: &str = "__tests__";

impl SectionKind {
    /// The row label for this section kind — currently just `"Tests"`
    /// for the one variant, but kept as a method (rather than inlining
    /// the string at each `row_view`/`ui` call site) so a second section
    /// kind's label lives in one place.
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
/// - `own_file_size_severity`: the severity of this file node's own
///   [`FileSizeWarning`] (ADR 0028), or `None` when the file is under
///   the watch threshold and for every non-file node. Paired with
///   `own_file_line_count`. Deliberately **not** merged upward: a
///   directory has no single severity of its own, only the aggregated
///   counts below (which are split by severity, so a mixed subtree of
///   `Warn` and `Split` files reads as `warn:N split:M` rather than
///   collapsing into one meaningless total).
/// - `own_file_line_count`: this file node's own line count, matching
///   the [`FileSizeWarning`] it carries. `None` when the file is under
///   the watch threshold and for every non-file node. Kept alongside
///   `own_file_size_severity` so the file row can render `lines:{N}`
///   without a second lookup back into the report.
/// - `file_size_warn_count`: bottom-up **count** of file nodes in this
///   subtree whose own severity is `FileSizeSeverity::Warn`. A directory
///   row displays this as `warn:N` (colored yellow) when nonzero — same
///   aggregation pattern as `fan_in` — and severity is kept split from
///   `file_size_split_count` so the reader sees "how many yellow" and
///   "how many red" separately.
/// - `file_size_split_count`: same as above for `FileSizeSeverity::Split`;
///   renders as `split:N` (colored red) on directory rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Badges {
    pub changed_symbols: usize,
    pub contract_changes: usize,
    pub fan_in: usize,
    pub own_file_size_severity: Option<FileSizeSeverity>,
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
        // `own_file_size_severity` / `own_file_line_count` are per-file
        // attributes, not aggregates — see this struct's doc comment.
        // The aggregates live in `file_size_warn_count` /
        // `file_size_split_count` above, split by severity.
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

    let file_size_by_path: HashMap<&str, (FileSizeSeverity, usize)> = report
        .file_size_warnings
        .iter()
        .map(|warning| {
            (
                warning.path.as_str(),
                (warning.severity, warning.line_count),
            )
        })
        .collect();

    let mut production = TreeBuilder::new(fan_in_by_id.clone(), file_size_by_path.clone());
    let mut tests = TreeBuilder::new(fan_in_by_id, file_size_by_path);

    // ADR 0035 Phase B: a *whole* test file (every symbol test code, or
    // matched by `LanguageSupport::is_test_path`) routes into `tests`
    // instead of `production` — a *mixed* file (some non-test symbols
    // alongside some test symbols) always stays in `production`
    // untouched, symbols and all, so this only ever affects whole
    // `FileReport`s, never splits one.
    for file in &report.files {
        if is_whole_test_file(&file.path, &file.symbols) {
            tests.insert_file(&file.path, &file.symbols);
        } else {
            production.insert_file(&file.path, &file.symbols);
        }
    }
    // `RemovedSymbol` carries no `is_test` flag (ADR 0035's Consequences:
    // no head-side AST context to classify it by) and a removed symbol's
    // file is never itself checked against `is_test_path` here — a
    // removed symbol therefore always stays in the production tree,
    // regardless of what the rest of its file's fate was.
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

    let mut section_roots = tests.finish().roots;
    if !section_roots.is_empty() {
        sort_alphabetically(&mut section_roots);
        let mut badges = Badges::default();
        for child in &section_roots {
            badges.merge(child.badges);
        }
        roots.push(TreeNode {
            kind: NodeKind::Section(SectionKind::Tests),
            path: TESTS_SECTION_PATH.to_string(),
            badges,
            children: section_roots,
            skip_reason: None,
            test_symbol_count: None,
        });
    }

    Tree { roots }
}

/// Whether `path`'s `FileReport` counts as a *whole* test file (ADR 0035
/// Phase B) rather than ordinary production code or a *mixed* file:
/// either `LanguageSupport::is_test_path` says the whole file is a test
/// file by convention (Go's `*_test.go`, etc.), or every symbol in
/// `symbols` has `is_test == true` — mirroring
/// `pipeline::partition_test_symbols`'s own "is this symbol excluded
/// under `--exclude-tests`" rule exactly, so Phase B's notion of "whole
/// test file" agrees with `rinkaku-core`'s. A file with an empty
/// `symbols` list (a pure rename with no path-convention match) is never
/// whole-test — same as `partition_test_symbols`'s own `had_symbols`
/// guard — since there is nothing to classify as test-only, and treating
/// an empty file as "whole test" would misfile a plain rename into the
/// Tests section for no reason.
fn is_whole_test_file(path: &str, symbols: &[rinkaku_core::extract::ExtractedSymbol]) -> bool {
    let is_test_path =
        rinkaku_core::language::language_for_path(path).is_some_and(|lang| lang.is_test_path(path));
    is_test_path || (!symbols.is_empty() && symbols.iter().all(|symbol| symbol.is_test))
}

/// Recursively re-sorts `nodes` (and every directory's children,
/// depth-first) A-Z by path, directories before files at each level —
/// same top-level ordering convention `crate::order::order_siblings`
/// uses for its own alphabetical mode, kept independent here rather than
/// calling into `crate::order` since the Tests section's ordering is
/// unconditional (ADR 0035 Phase B: never subject to the topological/
/// alphabetical toggle, so there is no `OrderMode`/`DirRank` to consult
/// at all — this is strictly simpler than `order_siblings`, not a
/// restricted call into it). `NodeKind::Symbol` children are left in
/// their original (extraction) order, matching `order_siblings`' own
/// "symbols are never reordered" rule — this function is only ever
/// called on section roots and directory children, never a file's own
/// symbol list.
fn sort_alphabetically(nodes: &mut [TreeNode]) {
    for node in nodes.iter_mut() {
        if matches!(node.kind, NodeKind::Dir) {
            sort_alphabetically(&mut node.children);
        }
    }
    nodes.sort_by(|a, b| {
        let a_is_dir = matches!(a.kind, NodeKind::Dir);
        let b_is_dir = matches!(b.kind, NodeKind::Dir);
        b_is_dir.cmp(&a_is_dir).then_with(|| a.path.cmp(&b.path))
    });
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
    /// `report.file_size_warnings`, keyed by path — the value is
    /// `(severity, line_count)` so a file node can populate both
    /// `own_file_size_severity` and `own_file_line_count` (and its
    /// per-severity contribution to the aggregate `file_size_warn_count` /
    /// `file_size_split_count`) in one lookup, while walking every source
    /// of file rows (`report.files`, `report.tests`, `report.skipped`).
    /// Built once in `build_tree`, same reasoning as `fan_in_by_id`.
    file_size_by_path: HashMap<&'a str, (FileSizeSeverity, usize)>,
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
    symbols: Vec<SymbolRef>,
    /// Set by `insert_skipped` — see `TreeNode::skip_reason`'s doc comment.
    skip_reason: Option<SkipReason>,
    /// Set by `insert_test_file` — see `TreeNode::test_symbol_count`'s doc
    /// comment.
    test_symbol_count: Option<usize>,
}

impl<'a> TreeBuilder<'a> {
    fn new(
        fan_in_by_id: HashMap<&'a str, usize>,
        file_size_by_path: HashMap<&'a str, (FileSizeSeverity, usize)>,
    ) -> Self {
        Self {
            root: DirBuilder::default(),
            fan_in_by_id,
            file_size_by_path,
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
            file_builder.symbols.push(SymbolRef {
                id: symbol.id.clone(),
                name: symbol.name.clone(),
                kind: symbol.kind,
                classification: symbol.classification,
                removed: false,
                is_test: symbol.is_test,
            });
        }
    }

    fn insert_removed(&mut self, path: &str, removed: &rinkaku_core::extract::RemovedSymbol) {
        let segments: Vec<&str> = path.split('/').collect();
        let file_builder = self.root.file_at(&segments);
        file_builder.symbols.push(SymbolRef {
            id: format!("{path}::{}", removed.name),
            name: removed.name.clone(),
            kind: removed.kind,
            classification: None,
            removed: true,
            // `RemovedSymbol` carries no `is_test` flag of its own — see
            // `SymbolRef::is_test`'s doc comment.
            is_test: false,
        });
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
            roots: self
                .root
                .into_nodes(String::new(), &self.fan_in_by_id, &self.file_size_by_path),
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
    /// see `symbol_badges`. `file_size_by_path` is threaded to file nodes,
    /// where a match seeds `own_file_size_severity` /
    /// `own_file_line_count` and the per-severity contribution to the
    /// aggregated `file_size_warn_count` / `file_size_split_count`.
    fn into_nodes(
        self,
        prefix: String,
        fan_in_by_id: &HashMap<&str, usize>,
        file_size_by_path: &HashMap<&str, (FileSizeSeverity, usize)>,
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
    file_size_by_path: &HashMap<&str, (FileSizeSeverity, usize)>,
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
    let children = dir.into_nodes(path.clone(), fan_in_by_id, file_size_by_path);
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
    file_size_by_path: &HashMap<&str, (FileSizeSeverity, usize)>,
) -> TreeNode {
    let path = join_path(prefix, &name);
    let (own_file_size_severity, own_file_line_count) = match file_size_by_path.get(path.as_str()) {
        Some((severity, line_count)) => (Some(*severity), Some(*line_count)),
        None => (None, None),
    };
    // A file with its own warning contributes 1 to the matching aggregate
    // count so an ancestor directory's `Badges::merge` sums the correct
    // per-severity totals (which `Badges::merge` does — the aggregate
    // fields — while it deliberately does not merge
    // `own_file_size_severity` / `own_file_line_count`, see `Badges`'
    // doc comment).
    let file_size_warn_count = usize::from(matches!(
        own_file_size_severity,
        Some(FileSizeSeverity::Warn)
    ));
    let file_size_split_count = usize::from(matches!(
        own_file_size_severity,
        Some(FileSizeSeverity::Split)
    ));
    let mut badges = Badges {
        own_file_size_severity,
        own_file_line_count,
        file_size_warn_count,
        file_size_split_count,
        ..Badges::default()
    };
    let children: Vec<TreeNode> = file
        .symbols
        .into_iter()
        .map(|symbol_ref| {
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

    TreeNode {
        kind: NodeKind::File,
        path,
        badges,
        children,
        skip_reason: file.skip_reason,
        test_symbol_count: file.test_symbol_count,
    }
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
        // File-size warnings are a per-file attribute (ADR 0028), not a
        // per-symbol one, so a symbol never contributes to any of the
        // file-size fields.
        own_file_size_severity: None,
        own_file_line_count: None,
        file_size_warn_count: 0,
        file_size_split_count: 0,
    }
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
