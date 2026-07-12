//! Directory tree view-model (ADR 0015): the TUI's entry view is the
//! directory tree of changed files, not the call-graph tree — nesting
//! depth conveys architecture, and each row carries aggregate badges.
//!
//! [`build_tree`] is a pure function from [`Report`] alone: same `Report`
//! in, same [`Tree`] out, no IO, no ordering decisions (ordering is a
//! separate concern, see `crate::order`).

use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::render::Report;
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
/// all if every symbol in that file was removed).
///
/// Construction is a pure function of `report` alone and deterministic:
/// files are visited in `report.files` order (already source order per
/// `pipeline::analyze_diff`), then `report.removed` for any additional
/// files/symbols not already covered, and directory chains are inserted in
/// that same discovery order.
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
            }
        })
        .collect();

    TreeNode {
        kind: NodeKind::File,
        path,
        badges,
        children,
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
    use rinkaku_core::render::FileReport;

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
                }],
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
                }],
            }],
        };
        let actual = build_tree(&report);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_collapse_directory_with_two_children() {
        let report = Report {
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
                    },
                    TreeNode {
                        kind: NodeKind::File,
                        path: "src/b.rs".to_string(),
                        badges: Badges::default(),
                        children: vec![],
                    },
                ],
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

        let tree = build_tree(&report);

        assert_eq!(1, tree.roots.len());
        let src = &tree.roots[0];
        assert_eq!(NodeKind::Dir, src.kind);
        assert_eq!("src", src.path);
        assert_eq!(2, src.children.len());
    }

    #[test]
    fn should_count_contract_change_for_signature_changed_symbol() {
        let report = Report {
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
                }],
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

        let tree = build_tree(&report);

        assert_eq!(1, tree.roots.len());
        let file_node = &tree.roots[0];
        assert_eq!(NodeKind::File, file_node.kind);
        assert_eq!(2, file_node.children.len());
        let expected_badges = Badges {
            changed_symbols: 1,
            contract_changes: 1,
            fan_in: 0,
        };
        assert_eq!(expected_badges, file_node.badges);
    }

    #[test]
    fn should_aggregate_badges_bottom_up_across_nested_directories() {
        let report = Report {
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
                }],
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
}
