//! What the tree cursor's current row resolves to for each of the right-hand
//! panes and for annotation targeting, split out of `app/mod.rs` (ADR 0028):
//! the `selected_*` read-only queries on [`App`], plus the view-model types
//! they return. Struct/enum definitions unrelated to cursor-row resolution
//! and `App`'s own construction/accessors stay in `app/mod.rs`/`app/state.rs`.

use crate::detail::{
    DetailView, DirDetail, FileDetail, build_detail, build_dir_detail, build_file_detail,
};
use crate::tree::NodeKind;
use rinkaku_core::render::Report;

use super::App;

/// The right-hand pane's content for the row currently under the cursor
/// (TUI iteration 2), unifying what used to be [`App::selected_detail`]'s
/// symbol-only contract: a directory or file row now has its own detail
/// too (`crate::detail::build_dir_detail`/`build_file_detail`), rather than
/// falling through to the placeholder every non-symbol row used to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectedDetail {
    Symbol(DetailView),
    Dir(DirDetail),
    File(FileDetail),
}

/// What [`App::selected_diff_target`] resolved the cursor's row to — plain
/// data describing which file the diff pane should slice hunks from;
/// `crate::ui` combines this with the raw diff text (via
/// `crate::diff_view`) at draw time.
///
/// Per ADR 0027 this always resolves to a file-scoped target, even for
/// symbol rows — the "which symbol is focused" information is carried by
/// [`App::selected_diff_focus`] on a separate accessor and applied by
/// `crate::run_app` as an auto-scroll offset, not by branching the diff
/// pane's shape here. The old `DiffTarget::Symbol` variant is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffTarget {
    File { path: String },
}

/// Which symbol (if any) the tree cursor is currently on for the diff
/// pane's benefit (ADR 0027 decision 2 + Consequences): `crate::run_app`
/// looks up this symbol's shaped section
/// (`crate::diff_shape::section_start_line_for_symbol`) and auto-scrolls
/// [`App::right_pane_scroll`] to that section's start whenever a new
/// selection triggers a diff-pane recompute. `None` on file/directory
/// rows and on removed symbol rows — those either have no symbol to
/// focus, or no line-range/graph identity to derive a section from.
///
/// `path` is redundant with [`DiffTarget::File`]'s own `path` (both come
/// from the same tree row), but is kept here so a caller with only a
/// `DiffFocus` in hand does not need to also thread a `DiffTarget`
/// through to know which file the focus belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFocus {
    pub path: String,
    pub symbol_id: String,
}

/// What [`App::selected_blast_radius_view`] resolved the cursor's row to
/// (ADR 0019, named "blast radius" per ADR 0023) — see that method's own
/// doc comment for the three-way split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlastRadiusSelection {
    NotApplicable,
    Empty { path: String },
    View(crate::blast_radius::BlastRadiusView),
}

/// What [`App::selected_row`] resolved the cursor's row to (ADR 0067) —
/// `crate::review_flow::derive_selection_snapshot`'s sole input for turning
/// a tree row into a [`crate::review::SelectionSnapshot`]. A distinct type
/// from [`crate::tree::NodeKind`] rather than exposing `NodeKind` itself:
/// `review_flow` needs only the identity fields relevant to an annotation
/// (path, symbol id/name), not the rest of `NodeKind`'s shape (badges,
/// children, kind-specific data no annotation cares about).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectedRow {
    Symbol {
        id: String,
    },
    RemovedSymbol {
        path: String,
        id: String,
        name: String,
    },
    File {
        path: String,
    },
    Dir {
        path: String,
    },
}

impl App {
    /// The detail-pane content for the row currently under the cursor
    /// (TUI iteration 2): a symbol's [`DetailView`], or a directory/file
    /// row's own [`DirDetail`]/[`FileDetail`] — `None` only when there are
    /// no rows at all, the cursor sits on a *removed* symbol (no detail to
    /// build, see `build_detail`'s doc comment), or `report`/`tree` no
    /// longer agree with each other (defensive — both should come from the
    /// same `App::new` call). `report` is threaded in per call rather than
    /// stored on `App` itself, since every `build_*` function here is
    /// already a cheap pure lookup and storing a whole `Report` on every
    /// `App` would duplicate data the caller (`crate::run`) already owns
    /// for the process's lifetime.
    pub fn selected_detail(&self, report: &Report) -> Option<SelectedDetail> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => {
                build_detail(report, &symbol_ref.id).map(SelectedDetail::Symbol)
            }
            NodeKind::Symbol(_) => None,
            NodeKind::Dir => {
                build_dir_detail(&self.tree, report, &row.node.path).map(SelectedDetail::Dir)
            }
            NodeKind::File => {
                build_file_detail(&self.tree, report, &row.node.path).map(SelectedDetail::File)
            }
            // A section's synthetic path never appears in `report.graph`,
            // so `build_dir_detail`'s cycle/fan-in lookups would find
            // nothing — falls back to the generic placeholder, same as a
            // removed symbol. Its children still get full detail once the
            // cursor moves onto them. A test group's synthetic path is the
            // same story.
            NodeKind::Section(_) | NodeKind::TestGroup { .. } => None,
        }
    }

    /// What the diff pane (TUI iteration 2, [`super::RightPane::Diff`])
    /// should slice out of the raw diff text for the row currently under
    /// the cursor: a file-scoped [`DiffTarget::File`] on both file rows and
    /// symbol rows (ADR 0027 decision 1 — the diff pane always renders the
    /// whole file, and "which symbol is focused" is carried on
    /// [`Self::selected_diff_focus`] alongside), or `None` on a directory
    /// row (a directory spans multiple files with no single diff to show —
    /// showing "every hunk under this directory" was considered and
    /// deferred, since it would just be the concatenation of every file's
    /// own diff, better browsed file by file). `None` also when there are
    /// no rows at all.
    ///
    /// `_report` is unused now that resolution needs only the tree row's
    /// own path (previously the symbol variant needed the line range from
    /// `report.files[..].symbols` — ADR 0027 folded that lookup into
    /// `crate::diff_shape` instead). Kept in the signature so the
    /// symmetry with [`Self::selected_detail`]/[`Self::selected_diff_focus`]
    /// stays intact for call sites that already thread `report` through.
    pub fn selected_diff_target(&self, _report: &Report) -> Option<DiffTarget> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => Some(DiffTarget::File {
                path: row.node.path.clone(),
            }),
            NodeKind::Symbol(_) => None,
            NodeKind::File => Some(DiffTarget::File {
                path: row.node.path.clone(),
            }),
            // A section spans multiple files, same reasoning as `Dir`
            // above — no single diff to show (ADR 0035 Phase B). A test
            // group's synthetic path is likewise not a real file path.
            NodeKind::Dir | NodeKind::Section(_) | NodeKind::TestGroup { .. } => None,
        }
    }

    /// The label [`crate::ui::diff_pane`] shows on line 1 of its pinned
    /// header for the row currently under the cursor: a present symbol's
    /// own name (paired with the path on a symbol row), or a
    /// file/skipped-file row's path (rendered bare) — mirrors
    /// [`Self::selected_diff_target`]'s row-kind scoping (present symbol
    /// or file only) so the header never names a row the pane would not
    /// actually render a diff for.
    pub fn selected_diff_header_name(&self) -> Option<&str> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => Some(symbol_ref.name.as_str()),
            NodeKind::File => Some(row.node.path.as_str()),
            _ => None,
        }
    }

    /// Which symbol the tree cursor currently focuses for the diff pane's
    /// auto-scroll (ADR 0027 decision 2 + Consequences): [`DiffFocus`] on a
    /// present symbol row, `None` on file/directory rows, on removed symbol
    /// rows (no graph identity to look up), or when there are no rows at
    /// all. `report` is threaded through only defensively — the focus id
    /// itself lives on the tree row already, but a caller wiring the focus
    /// into the shaped diff content must still know whether the id exists
    /// in `report.files[..].symbols`; when it does not (a mismatch between
    /// tree and report, "should not happen" but not enforceable at compile
    /// time), returning `None` here matches
    /// [`crate::diff_shape::section_start_line_for_symbol`]'s own "no
    /// section found" behavior so the diff pane simply does not auto-scroll
    /// rather than jumping to a stale offset.
    pub fn selected_diff_focus(&self, report: &Report) -> Option<DiffFocus> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        let NodeKind::Symbol(symbol_ref) = &row.node.kind else {
            return None;
        };
        if symbol_ref.removed {
            return None;
        }
        let known = report
            .files
            .iter()
            .find(|file| file.path == row.node.path)
            .is_some_and(|file| file.symbols.iter().any(|s| s.id == symbol_ref.id));
        if !known {
            return None;
        }
        Some(DiffFocus {
            path: row.node.path.clone(),
            symbol_id: symbol_ref.id.clone(),
        })
    }

    /// What the blast-radius pane ([`super::RightPane::BlastRadius`], ADR
    /// 0019/0023) should show for the row currently under the cursor:
    /// [`BlastRadiusSelection::View`] when the cursor sits on a directory or
    /// file row and at least one symbol falls under that row's path,
    /// [`BlastRadiusSelection::Empty`] for a directory/file row whose path
    /// matches no symbol at all (still a valid selection, just nothing to
    /// draw a tree from), or [`BlastRadiusSelection::NotApplicable`] on a
    /// symbol row or when there are no rows at all — mirroring
    /// `selected_diff_target`'s three-way split between "not this kind of
    /// row", "this kind of row but nothing to show", and "here is the
    /// content", except the blast-radius pane additionally needs to render
    /// its own "no symbols under `<path>`" message rather than reuse a
    /// diff-pane-style generic placeholder, hence the extra variant instead
    /// of `Option`.
    ///
    /// Not cached on `App` itself (ADR 0019's "recompute on toggle or
    /// cursor move while active, not per frame" stance) — but this method
    /// still recomputes from scratch (cost O(V+E), see
    /// `crate::blast_radius::build_blast_radius_view`'s own doc comment) on *every* call,
    /// so satisfying that stance is the caller's responsibility: `crate::run`'s
    /// event loop calls this once per handled key (when the blast-radius
    /// pane is active and the selection could have changed), caches the
    /// result, and hands the cached [`BlastRadiusSelection`] into
    /// `crate::ui::draw` — which must not call this method itself, since
    /// `terminal.draw` runs on every ~100ms idle poll tick as well as on an
    /// actual key press.
    pub fn selected_blast_radius_view(&self, report: &Report) -> BlastRadiusSelection {
        let rows = self.nav.rows(&self.tree);
        let Some(row) = rows.get(self.nav.cursor()) else {
            return BlastRadiusSelection::NotApplicable;
        };
        match &row.node.kind {
            // A section's synthetic path is not a real file-tree prefix,
            // so `build_blast_radius_view` would find nothing and report
            // `Empty` — misleading for "not applicable to this row kind",
            // so it's grouped with `Symbol` instead. A test group's
            // synthetic path has the same problem.
            NodeKind::Symbol(_) | NodeKind::Section(_) | NodeKind::TestGroup { .. } => {
                BlastRadiusSelection::NotApplicable
            }
            NodeKind::Dir | NodeKind::File => {
                match crate::blast_radius::build_blast_radius_view(report, &row.node.path) {
                    Some(view) => BlastRadiusSelection::View(view),
                    None => BlastRadiusSelection::Empty {
                        path: row.node.path.clone(),
                    },
                }
            }
        }
    }

    /// The id of the *present* (non-removed) symbol under the cursor, or
    /// `None` when the cursor is not on a symbol row at all, sits on a
    /// removed symbol (no graph presence to jump from — same reasoning as
    /// `selected_diff_target`'s own removed-symbol handling), or there are
    /// no rows at all. Used by `crate::run_app` to resolve `gd`/`gr`
    /// candidates (`crate::detail::symbol_mentions`) before calling back
    /// into `App` — see [`super::InputKey::GotoDefinition`]'s own doc
    /// comment on why that resolution cannot happen inside `App::handle_key`
    /// itself.
    pub fn selected_symbol_id(&self) -> Option<&str> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.removed => Some(symbol_ref.id.as_str()),
            _ => None,
        }
    }

    /// The tree row under the cursor, classified for annotation purposes
    /// (ADR 0067) — the wider counterpart to [`Self::selected_symbol_id`],
    /// which only ever reports a *present* symbol row and is kept as-is
    /// since navigation (`gd`/`gr`) has no use for a removed symbol or a
    /// `Dir`/`File` row. `None` on [`NodeKind::Section`]/[`NodeKind::TestGroup`]
    /// (synthetic groupings with no real file-tree path — ADR 0067's
    /// Decision 4) or when there are no rows at all.
    pub(crate) fn selected_row(&self) -> Option<SelectedRow> {
        let rows = self.nav.rows(&self.tree);
        let row = rows.get(self.nav.cursor())?;
        match &row.node.kind {
            NodeKind::Symbol(symbol_ref) if symbol_ref.removed => {
                Some(SelectedRow::RemovedSymbol {
                    path: row.node.path.clone(),
                    id: symbol_ref.id.clone(),
                    name: symbol_ref.name.clone(),
                })
            }
            NodeKind::Symbol(symbol_ref) => Some(SelectedRow::Symbol {
                id: symbol_ref.id.clone(),
            }),
            NodeKind::File => Some(SelectedRow::File {
                path: row.node.path.clone(),
            }),
            NodeKind::Dir => Some(SelectedRow::Dir {
                path: row.node.path.clone(),
            }),
            NodeKind::Section(_) | NodeKind::TestGroup { .. } => None,
        }
    }
}
