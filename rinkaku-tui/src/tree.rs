//! Directory tree view-model (ADR 0015): the TUI's entry view is the
//! directory tree of changed files, not the call-graph tree — nesting
//! depth conveys architecture, and each row carries aggregate badges.
//!
//! [`build_tree`] is a pure function from [`Report`] alone: same `Report`
//! in, same [`Tree`] out, no IO, no ordering decisions (ordering is a
//! separate concern, see `crate::order`).

use rinkaku_core::extract::{Classification, SymbolKind};
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
/// - `fan_in`: **sum** (not max) of `used_by.len()` for every hotspot
///   symbol contained in this node's subtree. Sum was chosen over max
///   because a directory containing several independently risky hotspots
///   should read as riskier than one containing a single hotspot with the
///   same peak fan-in — max would hide that difference.
///
///   This badge's `fan_in` is deliberately **not** the same computation
///   `crate::detail::build_detail` uses for a single symbol's `used_by`:
///   this badge only counts a symbol at all once it clears `Hotspot`'s own
///   fan-in >= 2 threshold (see `symbol_badges`'s doc comment), while the
///   detail pane's `used_by` reads `report.graph.edges` directly and so
///   also surfaces a fan-in of 0 or 1. A symbol with exactly one referrer
///   therefore shows up in its own detail view's `used_by` but contributes
///   nothing to any ancestor directory's `fan_in` badge here — expected,
///   not a bug, since the badge's whole purpose is to flag hotspots
///   specifically, not fan-in in general.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Badges {
    pub changed_symbols: usize,
    pub contract_changes: usize,
    pub fan_in: usize,
}

impl Badges {
    fn merge(&mut self, other: Badges) {
        self.changed_symbols += other.changed_symbols;
        self.contract_changes += other.contract_changes;
        self.fan_in += other.fan_in;
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
    /// exhaustive `match`es over `NodeKind` (dispatching detail/diff/pivot
    /// panes and sibling ordering) keep treating a skipped file exactly
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
        .hotspots
        .iter()
        .map(|hotspot| (hotspot.id.as_str(), hotspot.used_by.len()))
        .collect();

    let mut builder = TreeBuilder::new(fan_in_by_id);

    for file in &report.files {
        builder.insert_file(&file.path, &file.symbols);
    }
    for removed in &report.removed {
        builder.insert_removed(&removed.path, removed);
    }
    for test_file in &report.tests {
        builder.insert_test_file(&test_file.path, test_file.symbol_count);
    }
    for skipped in &report.skipped {
        if !matches!(skipped.reason, SkipReason::Generated) {
            builder.insert_skipped(&skipped.path, skipped.reason);
        }
    }

    builder.finish()
}

/// Intermediate mutable tree used only during construction — a
/// [`BTreeMap`]-backed trie keyed by path segment, so repeated
/// `insert_file`/`insert_removed` calls sharing a path prefix merge into
/// the same directory node instead of creating duplicates. Converted into
/// the immutable [`Tree`] (with badges aggregated and collapsing applied)
/// by [`TreeBuilder::finish`].
struct TreeBuilder<'a> {
    root: DirBuilder,
    /// `report.hotspots`, keyed by [`rinkaku_core::graph::NodeId`], so a
    /// symbol's fan-in badge can be looked up by id while walking
    /// `report.files` — built once in `build_tree` rather than per-symbol,
    /// since `report.hotspots` doesn't change during one `build_tree` call.
    fan_in_by_id: HashMap<&'a str, usize>,
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
    fn new(fan_in_by_id: HashMap<&'a str, usize>) -> Self {
        Self {
            root: DirBuilder::default(),
            fan_in_by_id,
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
            roots: self.root.into_nodes(String::new(), &self.fan_in_by_id),
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
    fn into_nodes(self, prefix: String, fan_in_by_id: &HashMap<&str, usize>) -> Vec<TreeNode> {
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
    let children = dir.into_nodes(path.clone(), fan_in_by_id);
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
) -> TreeNode {
    let path = join_path(prefix, &name);
    let mut badges = Badges::default();
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
/// `fan_in_by_id` is `report.hotspots` keyed by id (see `build_tree`): a
/// symbol not present there (fan-in < 2, or a removed symbol — never a
/// graph node) contributes zero fan-in, same as `Hotspot`'s own >= 2
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
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, RemovedSymbol};
    use rinkaku_core::graph::{Hotspot, SymbolGraph};
    use rinkaku_core::render::{FileReport, SkippedFile, TestFileSummary};

    fn symbol(id: &str, name: &str, kind: SymbolKind) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            signature: format!("fn {name}()"),
            range: LineRange { start: 1, end: 1 },
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
            hotspots: vec![],
            removed: vec![],
        }
    }

    #[test]
    fn should_build_empty_tree_when_report_has_no_files_and_no_removed() {
        let report = empty_report();

        let expected = Tree { roots: vec![] };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_build_flat_file_node_when_path_has_no_directory() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
            }],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::File,
                path: "lib.rs".to_string(),
                badges: Badges {
                    changed_symbols: 1,
                    contract_changes: 0,
                    fan_in: 0,
                },
                children: vec![TreeNode {
                    kind: NodeKind::Symbol(SymbolRef {
                        id: "lib.rs::foo".to_string(),
                        name: "foo".to_string(),
                        kind: SymbolKind::Function,
                        classification: None,
                        removed: false,
                    }),
                    path: "lib.rs".to_string(),
                    badges: Badges {
                        changed_symbols: 1,
                        contract_changes: 0,
                        fan_in: 0,
                    },
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: None,
                }],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_collapse_single_child_directory_chain_into_one_node() {
        // src/foo/bar/lib.rs — src, foo, bar each have exactly one child,
        // so all three collapse into one Dir node labeled "src/foo/bar".
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/foo/bar/lib.rs".to_string(),
                symbols: vec![],
            }],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::Dir,
                path: "src/foo/bar".to_string(),
                badges: Badges::default(),
                children: vec![TreeNode {
                    kind: NodeKind::File,
                    path: "src/foo/bar/lib.rs".to_string(),
                    badges: Badges::default(),
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: None,
                }],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_collapse_directory_with_two_children() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![
                FileReport {
                    path: "src/a.rs".to_string(),
                    symbols: vec![],
                },
                FileReport {
                    path: "src/b.rs".to_string(),
                    symbols: vec![],
                },
            ],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::Dir,
                path: "src".to_string(),
                badges: Badges::default(),
                children: vec![
                    TreeNode {
                        kind: NodeKind::File,
                        path: "src/a.rs".to_string(),
                        badges: Badges::default(),
                        children: vec![],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                    TreeNode {
                        kind: NodeKind::File,
                        path: "src/b.rs".to_string(),
                        badges: Badges::default(),
                        children: vec![],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                ],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_collapse_directory_that_has_own_file_alongside_subdirectory() {
        // src/ has both a direct file (mod.rs) and a subdirectory (foo/) —
        // src is not "just a chain" to reach foo, so it must stay a
        // separate node rather than collapsing with foo.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![
                FileReport {
                    path: "src/mod.rs".to_string(),
                    symbols: vec![],
                },
                FileReport {
                    path: "src/foo/bar.rs".to_string(),
                    symbols: vec![],
                },
            ],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::Dir,
                path: "src".to_string(),
                badges: Badges::default(),
                children: vec![
                    TreeNode {
                        kind: NodeKind::File,
                        path: "src/mod.rs".to_string(),
                        badges: Badges::default(),
                        children: vec![],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                    TreeNode {
                        kind: NodeKind::Dir,
                        path: "src/foo".to_string(),
                        badges: Badges::default(),
                        children: vec![TreeNode {
                            kind: NodeKind::File,
                            path: "src/foo/bar.rs".to_string(),
                            badges: Badges::default(),
                            children: vec![],
                            skip_reason: None,
                            test_symbol_count: None,
                        }],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                ],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_count_contract_change_for_signature_changed_symbol() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::SignatureChanged),
                    ..symbol("lib.rs::foo", "foo", SymbolKind::Function)
                }],
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        let expected = Badges {
            changed_symbols: 1,
            contract_changes: 1,
            fan_in: 0,
        };
        let actual = tree.roots[0].badges;

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_count_contract_change_for_body_only_symbol() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![ExtractedSymbol {
                    classification: Some(Classification::BodyOnly),
                    ..symbol("lib.rs::foo", "foo", SymbolKind::Function)
                }],
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        let expected = Badges {
            changed_symbols: 1,
            contract_changes: 0,
            fan_in: 0,
        };
        let actual = tree.roots[0].badges;

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_add_removed_symbol_as_marked_leaf_under_its_file_without_counting_as_changed() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![],
            removed: vec![RemovedSymbol {
                name: "gone".to_string(),
                kind: SymbolKind::Function,
                path: "lib.rs".to_string(),
                signature: "fn gone()".to_string(),
            }],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::File,
                path: "lib.rs".to_string(),
                badges: Badges {
                    changed_symbols: 0,
                    contract_changes: 1,
                    fan_in: 0,
                },
                children: vec![TreeNode {
                    kind: NodeKind::Symbol(SymbolRef {
                        id: "lib.rs::gone".to_string(),
                        name: "gone".to_string(),
                        kind: SymbolKind::Function,
                        classification: None,
                        removed: true,
                    }),
                    path: "lib.rs".to_string(),
                    badges: Badges {
                        changed_symbols: 0,
                        contract_changes: 1,
                        fan_in: 0,
                    },
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: None,
                }],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_merge_removed_symbol_into_existing_file_with_present_symbols() {
        // A file with one present (unchanged classification-wise) symbol
        // and one removed symbol must land under the same File node, not
        // create two separate entries for "lib.rs".
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
            }],
            removed: vec![RemovedSymbol {
                name: "gone".to_string(),
                kind: SymbolKind::Function,
                path: "lib.rs".to_string(),
                signature: "fn gone()".to_string(),
            }],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::File,
                path: "lib.rs".to_string(),
                badges: Badges {
                    changed_symbols: 1,
                    contract_changes: 1,
                    fan_in: 0,
                },
                children: vec![
                    TreeNode {
                        kind: NodeKind::Symbol(SymbolRef {
                            id: "lib.rs::foo".to_string(),
                            name: "foo".to_string(),
                            kind: SymbolKind::Function,
                            classification: None,
                            removed: false,
                        }),
                        path: "lib.rs".to_string(),
                        badges: Badges {
                            changed_symbols: 1,
                            contract_changes: 0,
                            fan_in: 0,
                        },
                        children: vec![],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                    TreeNode {
                        kind: NodeKind::Symbol(SymbolRef {
                            id: "lib.rs::gone".to_string(),
                            name: "gone".to_string(),
                            kind: SymbolKind::Function,
                            classification: None,
                            removed: true,
                        }),
                        path: "lib.rs".to_string(),
                        badges: Badges {
                            changed_symbols: 0,
                            contract_changes: 1,
                            fan_in: 0,
                        },
                        children: vec![],
                        skip_reason: None,
                        test_symbol_count: None,
                    },
                ],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    // NOTE: partial assert (root count, path, then only the aggregated
    // `Badges`) rather than a whole-`Tree` comparison — this test's only
    // concern is that bottom-up aggregation reaches the top of a
    // multi-level, multi-file subtree correctly; restating the full
    // "src/a/one.rs" and "src/b/two.rs" node structure (already pinned down
    // by other tests in this module, e.g.
    // `should_build_flat_file_node_when_path_has_no_directory`) would just
    // add noise without strengthening what this test is checking.
    #[test]
    fn should_aggregate_badges_bottom_up_across_nested_directories() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![
                FileReport {
                    path: "src/a/one.rs".to_string(),
                    symbols: vec![ExtractedSymbol {
                        classification: Some(Classification::SignatureChanged),
                        ..symbol("src/a/one.rs::x", "x", SymbolKind::Function)
                    }],
                },
                FileReport {
                    path: "src/b/two.rs".to_string(),
                    symbols: vec![symbol("src/b/two.rs::y", "y", SymbolKind::Function)],
                },
            ],
            ..empty_report()
        };

        let tree = build_tree(&report);

        assert_eq!(1, tree.roots.len());
        let src = &tree.roots[0];
        assert_eq!("src", src.path);
        let expected = Badges {
            changed_symbols: 2,
            contract_changes: 1,
            fan_in: 0,
        };
        assert_eq!(expected, src.badges);
    }

    #[test]
    fn should_keep_file_with_no_symbols_as_childless_file_node() {
        // A pure rename (FileReport with empty symbols) must still show up
        // as a File node with zero badges, not be dropped from the tree.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/renamed.rs".to_string(),
                symbols: vec![],
            }],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::Dir,
                path: "src".to_string(),
                badges: Badges::default(),
                children: vec![TreeNode {
                    kind: NodeKind::File,
                    path: "src/renamed.rs".to_string(),
                    badges: Badges::default(),
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: None,
                }],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_preserve_source_order_of_siblings_before_reordering() {
        // Discovery order in `report.files` must be preserved (reordering
        // is a separate concern handled by `crate::order`), even though the
        // builder uses a BTreeMap internally.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![
                FileReport {
                    path: "z.rs".to_string(),
                    symbols: vec![],
                },
                FileReport {
                    path: "a.rs".to_string(),
                    symbols: vec![],
                },
            ],
            ..empty_report()
        };

        let tree = build_tree(&report);

        let names: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(vec!["z.rs", "a.rs"], names);
    }

    #[test]
    fn should_set_fan_in_badge_from_matching_hotspot_and_aggregate_upward() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::shared", "shared", SymbolKind::Function)],
            }],
            hotspots: vec![Hotspot {
                id: "src/lib.rs::shared".to_string(),
                path: "src/lib.rs".to_string(),
                name: "shared".to_string(),
                used_by: vec!["a".to_string(), "b".to_string()],
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        // Fan-in of 2 (two referrers) must show on the symbol leaf and
        // aggregate up through File and Dir.
        let src = &tree.roots[0];
        assert_eq!("src", src.path);
        assert_eq!(2, src.badges.fan_in);
        let file_node = &src.children[0];
        assert_eq!(2, file_node.badges.fan_in);
        let symbol_node = &file_node.children[0];
        assert_eq!(2, symbol_node.badges.fan_in);
    }

    #[test]
    fn should_leave_fan_in_at_zero_when_symbol_has_no_matching_hotspot() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::solo", "solo", SymbolKind::Function)],
            }],
            hotspots: vec![],
            ..empty_report()
        };

        let tree = build_tree(&report);

        assert_eq!(0, tree.roots[0].badges.fan_in);
    }

    // Skipped-file tests: a file rinkaku could not extract symbols from
    // (unsupported language, binary, deleted) must still show up in the
    // tree, since otherwise it is invisible to a reviewer relying on the
    // TUI to see the whole PR (the user-reported gap this feature closes).

    #[test]
    fn should_add_skipped_file_as_childless_file_node_with_skip_reason() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            skipped: vec![SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::Dir,
                path: "assets".to_string(),
                badges: Badges::default(),
                children: vec![TreeNode {
                    kind: NodeKind::File,
                    path: "assets/logo.png".to_string(),
                    badges: Badges::default(),
                    children: vec![],
                    skip_reason: Some(rinkaku_core::render::SkipReason::Binary),
                    test_symbol_count: None,
                }],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_omit_generated_skip_reason_from_tree_by_default() {
        // Mirrors `render_markdown`'s own `SkipReason::Generated` filter
        // (ADR 0010/0011): a `.gitattributes`-declared or content-marked
        // generated file is already known-uninteresting, so it should not
        // clutter the TUI tree either.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            skipped: vec![SkippedFile {
                path: "Cargo.lock".to_string(),
                reason: rinkaku_core::render::SkipReason::Generated,
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        assert_eq!(Tree { roots: vec![] }, tree);
    }

    #[test]
    fn should_keep_non_generated_skip_reasons_when_mixed_with_a_generated_entry() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            skipped: vec![
                SkippedFile {
                    path: "Cargo.lock".to_string(),
                    reason: rinkaku_core::render::SkipReason::Generated,
                },
                SkippedFile {
                    path: "assets/logo.png".to_string(),
                    reason: rinkaku_core::render::SkipReason::Binary,
                },
            ],
            ..empty_report()
        };

        let tree = build_tree(&report);

        let paths: Vec<&str> = tree.roots.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(vec!["assets"], paths);
    }

    #[test]
    fn should_merge_skipped_file_into_existing_dir_alongside_analyzed_files() {
        // A skipped file sharing a directory with an already-analyzed file
        // must land in the same `Dir` node, not create a second "src" root.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![],
            }],
            skipped: vec![SkippedFile {
                path: "src/generated.pb.go".to_string(),
                reason: rinkaku_core::render::SkipReason::UnsupportedLanguage,
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        assert_eq!(1, tree.roots.len());
        let src = &tree.roots[0];
        assert_eq!("src", src.path);
        assert_eq!(2, src.children.len());
        // Source order: `report.files` is inserted before `report.skipped`
        // (see `build_tree`'s own doc comment), so the analyzed file comes
        // first even though "generated.pb.go" sorts first alphabetically.
        let paths: Vec<&str> = src.children.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(vec!["src/lib.rs", "src/generated.pb.go"], paths);
    }

    // Whole-test-file tests: a file whose changed symbols were *all* test
    // code has no `FileReport` in `report.files` at all
    // (`pipeline::partition_test_symbols`'s doc comment) — only a
    // `TestFileSummary` in `report.tests`. Without surfacing that summary
    // into the tree, such a file is invisible to a reviewer, the same gap
    // as a skipped file.

    #[test]
    fn should_add_whole_test_file_as_childless_file_node_with_symbol_count() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            tests: vec![TestFileSummary {
                path: "src/lib_test.go".to_string(),
                symbol_count: 3,
            }],
            ..empty_report()
        };

        let expected = Tree {
            roots: vec![TreeNode {
                kind: NodeKind::Dir,
                path: "src".to_string(),
                badges: Badges::default(),
                children: vec![TreeNode {
                    kind: NodeKind::File,
                    path: "src/lib_test.go".to_string(),
                    badges: Badges::default(),
                    children: vec![],
                    skip_reason: None,
                    test_symbol_count: Some(3),
                }],
                skip_reason: None,
                test_symbol_count: None,
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_merge_test_file_into_existing_dir_alongside_analyzed_files() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("src/lib.rs::foo", "foo", SymbolKind::Function)],
            }],
            tests: vec![TestFileSummary {
                path: "src/lib_test.rs".to_string(),
                symbol_count: 2,
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        assert_eq!(1, tree.roots.len());
        let src = &tree.roots[0];
        assert_eq!("src", src.path);
        assert_eq!(2, src.children.len());
        let paths: Vec<&str> = src.children.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(vec!["src/lib.rs", "src/lib_test.rs"], paths);
    }

    #[test]
    fn should_not_set_test_symbol_count_or_skip_reason_on_an_ordinary_file() {
        // Regression guard: an ordinary analyzed file must keep both new
        // fields at `None`, not accidentally inherit a stale default from
        // whatever `FileBuilder` construction path is taken.
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![],
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        assert_eq!(None, tree.roots[0].skip_reason);
        assert_eq!(None, tree.roots[0].test_symbol_count);
    }

    // `pipeline::analyze_diff` never produces a `Report` where the same path
    // appears in both `files`/`skipped` or both `tests`/`skipped` (see
    // `TreeBuilder::insert_file`/`insert_test_file`/`insert_skipped`'s own
    // doc comments), so `#[cfg(debug_assertions)]` keeps these panic-path
    // tests out of release builds, matching the `debug_assert!`s themselves
    // — they only guard a caller contract, not a condition `build_tree`
    // needs to handle gracefully at runtime. `files`/`tests` overlapping on
    // the same path, in contrast, is a *valid* `analyze_diff` output (a
    // mixed file) — see `should_keep_real_symbols_when_file_is_also_in_tests`
    // below, not a panic case.
    // `build_tree` visits `report.files` before `report.skipped` (its own
    // doc comment's discovery order), so this hits `insert_skipped`'s own
    // assert, not `insert_file`'s — `insert_file` only guards against a
    // path *already* marked skipped when files runs, which is not yet true
    // here.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "report.skipped must not overlap report.files/report.tests")]
    fn should_panic_when_the_same_path_appears_in_files_and_skipped() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
            }],
            skipped: vec![SkippedFile {
                path: "lib.rs".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            ..empty_report()
        };

        build_tree(&report);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "report.skipped must not overlap report.files/report.tests")]
    fn should_panic_when_the_same_path_appears_in_tests_and_skipped() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            tests: vec![TestFileSummary {
                path: "lib.rs".to_string(),
                symbol_count: 1,
            }],
            skipped: vec![SkippedFile {
                path: "lib.rs".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            ..empty_report()
        };

        build_tree(&report);
    }

    // Regression test (post-rebase integration check): `lib.rs` in both
    // `report.files` (some real symbols) and `report.tests` (a test count
    // for the rest) is exactly what `pipeline::partition_test_symbols`
    // produces for a mixed file — e.g. a Rust file with production
    // functions changed alongside its own `#[cfg(test)] mod tests` in the
    // same diff (this crate's own `rinkaku-tui/src/app.rs` hit this in a
    // live dogfood run). `build_tree` must keep both pieces of information
    // on the one `TreeNode` rather than panicking or silently dropping
    // either half.
    #[test]
    fn should_keep_real_symbols_when_file_is_also_in_tests() {
        let report = Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo", SymbolKind::Function)],
            }],
            tests: vec![TestFileSummary {
                path: "lib.rs".to_string(),
                symbol_count: 3,
            }],
            ..empty_report()
        };

        let tree = build_tree(&report);

        assert_eq!(1, tree.roots[0].children.len());
        assert_eq!(Some(3), tree.roots[0].test_symbol_count);
        assert_eq!(None, tree.roots[0].skip_reason);
    }
}
