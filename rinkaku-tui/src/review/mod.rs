//! Review notes (ADR 0048): a location-anchored `Note` primitive plus a
//! pure state machine (`ReviewState`) for composing, listing, and exporting
//! them, decoupled from the rest of the TUI through the narrow
//! `SelectionSnapshot` input and the `Vec<Note>`/rendered-`String` output
//! (this module's own "own state, narrow input" boundary — see the ADR's
//! Module boundary decision).
//!
//! `review` never holds a `&Report` or reaches into `App`'s tree/nav
//! fields; `crate::app`/`crate::lib` derive a [`SelectionSnapshot`] from
//! whatever the cursor currently points at and hand it in. Every side
//! effect (posting a GitHub review, writing the clipboard) sits behind the
//! [`ports::ReviewSubmitter`]/[`ports::ClipboardSink`] traits, implemented
//! and wired up by the `rinkaku` binary crate, never called from here.

pub mod ports;
mod render;

pub use render::{render_agent_packet, render_review_comments};

/// A destination-neutral note attached to a location in the diff (ADR
/// 0048's primitive): nothing about a `Note` says who will eventually read
/// it — that decision is deferred to export time, per sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub location: NoteLocation,
    pub body: String,
    pub signature: Option<String>,
}

/// Where a [`Note`] is anchored: a file path, the symbol it was taken
/// against (if any), the symbol's own new-side line range, and the
/// GitHub-comment anchor within that range (the first hunk-intersecting
/// contiguous run — see [`crate::review::render_review_comments`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteLocation {
    pub path: String,
    pub symbol_id: Option<String>,
    pub symbol_name: Option<String>,
    /// The symbol's own new-side line range, 1-based inclusive.
    pub range: Option<(usize, usize)>,
    /// The new-side hunk/`range` intersection's first contiguous run,
    /// 1-based inclusive — GitHub's review API only accepts inline
    /// comments on lines that are part of the PR's diff, so this is the
    /// anchor [`crate::review::render_review_comments`] posts against.
    pub anchor: Option<(usize, usize)>,
}

/// What the cursor pointed at when `n` (compose) was pressed — the sole
/// channel by which [`ReviewState`] learns what the reviewer is
/// annotating (ADR 0048's Input boundary decision). Derived by
/// `crate::lib`/`crate::app` from the tree cursor plus the diff hunks
/// already parsed for the session; `review` never derives this itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionSnapshot {
    pub path: String,
    pub symbol_id: Option<String>,
    pub symbol_name: Option<String>,
    pub range: Option<(usize, usize)>,
    pub anchor: Option<(usize, usize)>,
    pub signature: Option<String>,
}

impl From<SelectionSnapshot> for NoteLocation {
    fn from(snapshot: SelectionSnapshot) -> Self {
        Self {
            path: snapshot.path,
            symbol_id: snapshot.symbol_id,
            symbol_name: snapshot.symbol_name,
            range: snapshot.range,
            anchor: snapshot.anchor,
        }
    }
}

/// A PR's identity, enough to post a review against it (ADR 0048 sink A):
/// assembled once in `main.rs` after `run_analysis` succeeds from
/// `PrInfo`/`PrArg`/`git_remote_origin_url`, `None` for every non-`--pr`
/// input mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrContext {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub head_sha: String,
}

/// The verdict a reviewer picks when exporting to sink A (GitHub PR
/// review), mirroring GitHub's own pending-review submit dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

/// One [`Note`] rendered for sink A (GitHub PR review comments) — plain
/// data, not a `gh api` request shape, so [`crate::review::render_review_comments`]
/// stays testable without a JSON dependency and the `rinkaku` binary crate
/// decides the wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedComment {
    pub path: String,
    pub line: usize,
    pub start_line: Option<usize>,
    pub body: String,
}

/// Which sink an export targets, resolved once the reviewer confirms the
/// export menu (and, for [`Self::GithubReview`], the verdict menu) —
/// [`ReviewState::take_pending_export`] hands this to `crate::lib::run_app`
/// to actually perform, since `review` itself never touches a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportRequest {
    GithubReview(Verdict),
    Clipboard,
}

/// The review overlay's current mode — which of the compose/list/export/
/// verdict surfaces (if any) is on screen, and that surface's own
/// transient state (an in-progress compose buffer, a list/menu cursor).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ReviewMode {
    #[default]
    Idle,
    Compose {
        snapshot: SelectionSnapshot,
        buffer: String,
    },
    List {
        cursor: usize,
    },
    ExportMenu {
        cursor: usize,
    },
    VerdictMenu {
        cursor: usize,
    },
}

/// The export menu's selectable entries, in display order — indexed by
/// [`ReviewMode::ExportMenu`]'s `cursor`. `Github` is omitted from the
/// menu entirely (not shown disabled) when no [`PrContext`] is available,
/// per ADR 0048's "no implicit fallback" decision — see
/// [`ReviewState::open_export_menu`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportMenuEntry {
    Github,
    Clipboard,
}

/// The verdict menu's selectable entries, in display order.
const VERDICT_ENTRIES: [Verdict; 3] = [Verdict::Approve, Verdict::RequestChanges, Verdict::Comment];

/// The review feature's own state (ADR 0048's Module boundary decision):
/// the accumulated notes, which overlay (if any) is open, a change counter
/// consulted by `crate::lib::run_app`'s `NoteMarkers` cache-on-change gate,
/// and a pending export request/status message for `crate::lib::run_app`
/// to act on and report back through. Every method here is a pure state
/// transition — no IO, no port calls.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewState {
    notes: Vec<Note>,
    mode: ReviewMode,
    /// Incremented on every mutation to `notes` — `crate::lib::run_app`'s
    /// `NoteMarkers` cache-on-change gate compares this against the value
    /// it last recomputed from, mirroring
    /// `should_recompute_blast_radius_selection`'s own contract (ADR
    /// 0048's Rendering boundary decision: `NoteMarkers` must not be
    /// derived per frame).
    revision: u64,
    pending_export: Option<ExportRequest>,
    last_status: Option<String>,
}

impl ReviewState {
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn mode(&self) -> &ReviewMode {
        &self.mode
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn last_status(&self) -> Option<&str> {
        self.last_status.as_deref()
    }

    /// Opens the compose overlay over `snapshot` — called by
    /// `crate::lib::run_app` when `n` is pressed and a snapshot could be
    /// derived from the cursor (`derive_selection_snapshot` returned
    /// `Some`); a `None` snapshot never reaches this method at all, since
    /// `run_app` special-cases `InputKey::NoteCompose` before dispatch
    /// (ADR 0048's touch-point (a): the one key that needs data
    /// `App::handle_key` cannot provide).
    pub fn begin_compose(mut self, snapshot: SelectionSnapshot) -> Self {
        self.mode = ReviewMode::Compose {
            snapshot,
            buffer: String::new(),
        };
        self
    }

    /// Appends `c` to the compose buffer — a no-op outside
    /// [`ReviewMode::Compose`].
    pub fn push_char(mut self, c: char) -> Self {
        if let ReviewMode::Compose { buffer, .. } = &mut self.mode {
            buffer.push(c);
        }
        self
    }

    /// Removes the last character from the compose buffer — a no-op
    /// outside [`ReviewMode::Compose`] or on an already-empty buffer.
    pub fn backspace(mut self) -> Self {
        if let ReviewMode::Compose { buffer, .. } = &mut self.mode {
            buffer.pop();
        }
        self
    }

    /// Confirms the in-progress compose: appends a [`Note`] built from the
    /// snapshot and buffer and returns to [`ReviewMode::Idle`], unless the
    /// buffer is empty or whitespace-only, in which case composing is
    /// simply abandoned (no note is added) — mirrors [`Self::cancel_compose`]
    /// for a blank buffer, since an empty note carries nothing worth
    /// recording. A no-op outside [`ReviewMode::Compose`].
    pub fn confirm_compose(mut self) -> Self {
        if let ReviewMode::Compose { snapshot, buffer } = self.mode {
            if !buffer.trim().is_empty() {
                let signature = snapshot.signature.clone();
                self.notes.push(Note {
                    location: snapshot.into(),
                    body: buffer,
                    signature,
                });
                self.revision += 1;
            }
            self.mode = ReviewMode::Idle;
        }
        self
    }

    /// Abandons the in-progress compose without adding a note — a no-op
    /// outside [`ReviewMode::Compose`].
    pub fn cancel_compose(mut self) -> Self {
        if matches!(self.mode, ReviewMode::Compose { .. }) {
            self.mode = ReviewMode::Idle;
        }
        self
    }

    /// Opens the notes list overlay (`N`), cursor on the first note.
    pub fn open_list(mut self) -> Self {
        self.mode = ReviewMode::List { cursor: 0 };
        self
    }

    /// Closes whichever overlay is open, discarding any in-progress
    /// compose/menu state, and returns to [`ReviewMode::Idle`].
    pub fn close(mut self) -> Self {
        self.mode = ReviewMode::Idle;
        self
    }

    /// Moves the active list/menu cursor up by one (clamped, not
    /// wrapping) — a no-op in [`ReviewMode::Idle`]/[`ReviewMode::Compose`].
    pub fn list_up(mut self) -> Self {
        match &mut self.mode {
            ReviewMode::List { cursor }
            | ReviewMode::ExportMenu { cursor }
            | ReviewMode::VerdictMenu { cursor } => *cursor = cursor.saturating_sub(1),
            ReviewMode::Idle | ReviewMode::Compose { .. } => {}
        }
        self
    }

    /// Moves the active list/menu cursor down by one, clamped to the
    /// relevant list's length — a no-op in [`ReviewMode::Idle`]/
    /// [`ReviewMode::Compose`].
    ///
    /// The `ExportMenu` clamp uses the menu's maximum possible length (both
    /// sinks present) rather than the `sink_a_available`-filtered length
    /// [`Self::confirm_export`] resolves against: this module carries no
    /// `PrContext` of its own (the Input/Output boundary keeps it plain
    /// data), so it cannot know here whether sink A is on the menu.
    /// Overshooting by one slot when sink A is absent is harmless —
    /// [`Self::confirm_export`]'s own `entries.get(cursor)` falls through
    /// to its `None` arm (closing the menu) rather than panicking or
    /// selecting the wrong entry.
    pub fn list_down(mut self) -> Self {
        match &mut self.mode {
            ReviewMode::List { cursor } => {
                *cursor = (*cursor + 1).min(self.notes.len().saturating_sub(1));
            }
            ReviewMode::ExportMenu { cursor } => {
                let max_len = export_menu_entries(true).len();
                *cursor = (*cursor + 1).min(max_len.saturating_sub(1));
            }
            ReviewMode::VerdictMenu { cursor } => {
                *cursor = (*cursor + 1).min(VERDICT_ENTRIES.len().saturating_sub(1));
            }
            ReviewMode::Idle | ReviewMode::Compose { .. } => {}
        }
        self
    }

    /// Deletes the note the list cursor currently points at — a no-op
    /// outside [`ReviewMode::List`] or when the list is empty.
    pub fn delete_selected(mut self) -> Self {
        if let ReviewMode::List { cursor } = self.mode
            && cursor < self.notes.len()
        {
            self.notes.remove(cursor);
            self.revision += 1;
            let new_len = self.notes.len();
            self.mode = ReviewMode::List {
                cursor: cursor.min(new_len.saturating_sub(1)),
            };
        }
        self
    }

    /// Opens the export menu (`x` from the notes list) — a no-op outside
    /// [`ReviewMode::List`].
    pub fn open_export_menu(mut self) -> Self {
        if matches!(self.mode, ReviewMode::List { .. }) {
            self.mode = ReviewMode::ExportMenu { cursor: 0 };
        }
        self
    }

    /// Confirms the export menu's highlighted entry: [`ExportMenuEntry::Github`]
    /// (only ever selectable when `sink_a_available`, i.e. a [`PrContext`]
    /// exists — ADR 0048's "sink A is simply absent, never disabled" rule)
    /// opens [`ReviewMode::VerdictMenu`] next; [`ExportMenuEntry::Clipboard`]
    /// sets [`ExportRequest::Clipboard`] as the pending export and returns
    /// to [`ReviewMode::Idle`]. A no-op outside [`ReviewMode::ExportMenu`].
    pub fn confirm_export(mut self, sink_a_available: bool) -> Self {
        if let ReviewMode::ExportMenu { cursor } = self.mode {
            let entries = export_menu_entries(sink_a_available);
            match entries.get(cursor) {
                Some(ExportMenuEntry::Github) => {
                    self.mode = ReviewMode::VerdictMenu { cursor: 0 };
                }
                Some(ExportMenuEntry::Clipboard) => {
                    self.pending_export = Some(ExportRequest::Clipboard);
                    self.mode = ReviewMode::Idle;
                }
                None => {
                    self.mode = ReviewMode::Idle;
                }
            }
        }
        self
    }

    /// Confirms the verdict menu's highlighted entry, setting
    /// [`ExportRequest::GithubReview`] as the pending export and returning
    /// to [`ReviewMode::Idle`]. A no-op outside [`ReviewMode::VerdictMenu`].
    pub fn confirm_verdict(mut self) -> Self {
        if let ReviewMode::VerdictMenu { cursor } = self.mode
            && let Some(&verdict) = VERDICT_ENTRIES.get(cursor)
        {
            self.pending_export = Some(ExportRequest::GithubReview(verdict));
            self.mode = ReviewMode::Idle;
        }
        self
    }

    /// Takes the pending export request, if any, leaving `None` behind —
    /// `crate::lib::run_app` calls this once per handled key to decide
    /// whether to perform an export this iteration.
    pub fn take_pending_export(&mut self) -> Option<ExportRequest> {
        self.pending_export.take()
    }

    /// Sets the status message shown in the notes-list overlay (an export
    /// result, success or failure) — called by `crate::lib::run_app` after
    /// performing an export.
    pub fn set_status(mut self, message: impl Into<String>) -> Self {
        self.last_status = Some(message.into());
        self
    }
}

/// The export menu's entries given whether sink A is available — extracted
/// so [`ReviewState::confirm_export`]/[`ReviewState::list_down`] share one
/// definition of "what's on the menu, in what order".
fn export_menu_entries(sink_a_available: bool) -> Vec<ExportMenuEntry> {
    let mut entries = Vec::new();
    if sink_a_available {
        entries.push(ExportMenuEntry::Github);
    }
    entries.push(ExportMenuEntry::Clipboard);
    entries
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
