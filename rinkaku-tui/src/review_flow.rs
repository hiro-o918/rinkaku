//! ADR 0048 review-notes integration glue (split out of `lib.rs`, which had
//! grown past the file-size threshold, ADR 0028): the pieces of
//! `crate::run_app`'s event loop that are specifically about composing,
//! exporting, and caching review notes, as opposed to the loop's general
//! dispatch machinery (`dispatch_non_source_key`, the diff-pane/blast-radius
//! recompute gates, etc., which stay in `lib.rs`). Every function here is
//! pure or, for [`perform_export`], calls out to a port passed in by the
//! caller — none of them touch a live `ratatui::DefaultTerminal` directly,
//! which is what makes them unit-testable in isolation from `run_app`
//! itself.

use crate::app::{App, InputKey, Screen};
use crate::{ReviewPorts, diff_view, review};
use rinkaku_core::render::Report;

/// Applies [`InputKey::NoteCompose`] given the [`review::SelectionSnapshot`]
/// `crate::run_app` already derived from the cursor (that derivation needs
/// `report`/the parsed diff hunks, which `App::handle_key` has no access
/// to — `InputKey::NoteCompose`'s own doc comment). Always routes through
/// `App::handle_key` first, even on a `None` snapshot (cursor not on a
/// present symbol row, or on the source screen): `handle_key`'s own
/// `NoteCompose` match arm is a no-op stub, but what matters is its
/// unconditional `pending_prefix` clear at the top of the function — the
/// same "call `handle_key` for its clear even when its own arm does
/// nothing" contract `crate::dispatch_non_source_key`'s `GotoDefinition`/
/// `GotoReferences` arm documents for itself. A `Some` snapshot opens the
/// compose overlay after that call; `None` leaves `review` untouched.
pub(crate) fn dispatch_note_compose_key(
    app: App,
    snapshot: Option<review::SelectionSnapshot>,
) -> App {
    let app = app.handle_key(InputKey::NoteCompose);
    match snapshot {
        Some(snapshot) => {
            let review = app.review().clone().begin_compose(snapshot);
            app.with_review(review)
        }
        None => app,
    }
}

/// Whether `crate::run_app`'s event loop should recompute
/// [`crate::note_markers::NoteMarkers`] this key, mirroring
/// `crate::should_recompute_blast_radius_selection`'s/
/// `crate::should_recompute_diff_pane_content`'s own change-gated-cache
/// contract (ADR 0048's Rendering boundary decision: this derivation must
/// not run inside `ui::draw`, since that runs on every ~100ms idle poll
/// tick, not only on a key press). `true` only when `review`'s note set
/// actually changed since the last recompute — compares `revision` rather
/// than gating on screen/pane the way the blast-radius/diff-pane gates do,
/// since note markers are relevant on every row/pane, not just one right
/// pane's own active mode.
pub(crate) fn should_recompute_note_markers(app: &App, last_revision: u64) -> bool {
    app.review().revision() != last_revision
}

/// The summary line posted alongside every GitHub PR review sink A submits
/// (ADR 0048) — every review is submitted with the same fixed summary,
/// since the individual notes themselves carry the substantive content as
/// inline comments; there is no per-session reviewer-authored summary in
/// v1.
const REVIEW_SUMMARY: &str = "Review notes posted via rinkaku.";

/// Performs `export` against the matching port in `ports` (ADR 0048's
/// Output boundary decision: `review` itself never calls a port, only
/// `crate::run_app` does, once per handled key that produced a pending
/// export) and folds the result into `review`'s status message.
///
/// [`review::ExportRequest::GithubReview`] is only ever produced by
/// [`review::ReviewState::confirm_verdict`], reachable only through
/// [`review::ReviewState::confirm_export`]'s own `sink_a_available`-gated
/// branch (`App::handle_review_key`'s own `ExportMenu` arm passes
/// `app.review_sink_a_available`) — so `ports.submitter` being `None` here
/// would mean that gate was bypassed; handled defensively (a status
/// message, not a panic) rather than trusted blindly, matching this
/// crate's existing practice of not trusting an invariant across a module
/// boundary (e.g. `App::jump_to_symbol`'s own doc comment on the same
/// judgment call).
pub(crate) fn perform_export(
    review: review::ReviewState,
    ports: &ReviewPorts<'_>,
    export: review::ExportRequest,
) -> review::ReviewState {
    match export {
        review::ExportRequest::GithubReview(verdict) => {
            let Some(submitter) = ports.submitter else {
                return review.set_status("error: no PR context available to post a review");
            };
            let Some(pr_context) = &ports.pr_context else {
                return review.set_status("error: no PR context available to post a review");
            };
            let comments = review::render_review_comments(review.notes());
            match submitter.submit_review(pr_context, verdict, REVIEW_SUMMARY, &comments) {
                Ok(()) => review.set_status(format!(
                    "posted {} review comment(s) to PR #{}",
                    comments.len(),
                    pr_context.number
                )),
                Err(message) => review.set_status(format!("error posting review: {message}")),
            }
        }
        review::ExportRequest::Clipboard => {
            let packet = review::render_agent_packet(review.notes());
            let result = ports.clipboard.copy(&packet);
            let status = clipboard_export_status(&packet, result);
            review.set_status(status)
        }
    }
}

/// A conservative guard on the OSC 52 packet's *raw* byte length (ADR
/// 0048), below common terminal-side payload caps (~100KB is a frequently
/// cited limit, e.g. iTerm2/xterm.js) even after base64 inflates the wire
/// payload to ~4/3 of this — terminals that enforce such a cap tend to
/// silently drop or truncate an oversized OSC 52 sequence rather than
/// erroring, so `Osc52Clipboard::copy`-equivalent ports (`rinkaku`'s own
/// `Osc52Clipboard`) have no way to detect the failure themselves; this
/// guard exists purely to warn the reviewer before that silent failure
/// bites them.
pub(crate) const OSC52_SIZE_GUARD_BYTES: usize = 48 * 1024;

/// Folds sink B's [`review::ports::ClipboardSink::copy`] result into a
/// status message: the port's own error message on `Err`, otherwise a
/// plain success note — or, when `packet` exceeds
/// [`OSC52_SIZE_GUARD_BYTES`], the same success note plus a warning that
/// the terminal may have silently dropped or truncated it, with a manual-
/// copy fallback suggestion. The copy itself already happened by the time
/// this runs; the guard only changes the *message*, never whether the copy
/// is attempted.
pub(crate) fn clipboard_export_status(packet: &str, result: Result<(), String>) -> String {
    match result {
        Ok(()) => {
            let base = "copied review notes to clipboard via OSC 52 (terminal support required)";
            if packet.len() > OSC52_SIZE_GUARD_BYTES {
                format!(
                    "{base} — packet is {} bytes, which may exceed the terminal's OSC 52 limit; \
                     copy manually if the paste looks truncated",
                    packet.len()
                )
            } else {
                base.to_string()
            }
        }
        Err(message) => format!("error copying to clipboard: {message}"),
    }
}

/// Derives a [`review::SelectionSnapshot`] from whatever the tree cursor
/// currently points at (ADR 0048's Input boundary decision) — the sole
/// channel by which `review` learns what the reviewer is annotating.
/// `crate::run_app` calls this when [`InputKey::NoteCompose`] is pressed,
/// since `App::handle_key` itself has no access to `report`/the parsed diff
/// hunks (mirroring `InputKey::Source`'s own "IO/derivation stays outside
/// `App`" precedent).
///
/// `None` on [`Screen::Source`] (composing against a source-view line is
/// out of v1's scope) and on any row that is not a present symbol
/// (`app::NodeKind::Dir`/`File`/`Section`/`TestGroup`, or a removed
/// symbol) — v1 only supports symbol-anchored notes (module doc comment on
/// `crate::review`), matching `App::selected_symbol_id`'s own row-kind
/// scoping.
///
/// The anchor is the first contiguous new-side run where the symbol's own
/// range intersects a diff hunk touching `path` — GitHub's review API only
/// accepts inline comments on lines that are part of the PR's diff, so
/// this is what [`review::render_review_comments`] posts against. `None`
/// when no hunk intersects the symbol's range at all (e.g. the symbol
/// itself is unchanged but was pulled into view via dependency
/// expansion) — the note still gets a location (`range`), just no
/// GitHub-postable anchor; [`review::render_review_comments`] falls back
/// to `range` in that case.
pub(crate) fn derive_selection_snapshot(
    app: &App,
    report: &Report,
    diff_files: &[diff_view::FileHunks],
) -> Option<review::SelectionSnapshot> {
    if !matches!(app.screen(), Screen::Entry) {
        return None;
    }
    let symbol_id = app.selected_symbol_id()?;
    let (path, symbol) = report.files.iter().find_map(|file| {
        file.symbols
            .iter()
            .find(|s| s.id == symbol_id)
            .map(|s| (file.path.as_str(), s))
    })?;
    let range = (symbol.range.start, symbol.range.end);
    let anchor = diff_view::file_hunks(diff_files, path)
        .and_then(|file_hunks| first_anchor_run(file_hunks, range));

    Some(review::SelectionSnapshot {
        path: path.to_string(),
        symbol_id: Some(symbol.id.clone()),
        symbol_name: Some(symbol.name.clone()),
        range: Some(range),
        anchor,
        signature: Some(symbol.signature.clone()),
    })
}

/// The first contiguous new-side line run where `range` (a symbol's own
/// 1-based inclusive line range) intersects one of `file_hunks`' hunks —
/// [`derive_selection_snapshot`]'s own anchor computation, extracted as a
/// pure function so the "first run" rule is unit-testable in isolation
/// from `Report`/`App`.
///
/// Hunks are walked in file order (already the order `diff_view::parse_diff_hunks`
/// produces them in) and the *first* intersecting hunk's own clamped
/// overlap with `range` is returned — not the union of every intersecting
/// hunk — since ADR 0048 asks for "the first hunk-intersecting contiguous
/// run", not the full set (a symbol whose range spans several
/// non-adjacent hunks has no single contiguous GitHub-postable range
/// anyway; the first run is a deliberately simple v1 choice, not an
/// attempt at completeness).
pub(crate) fn first_anchor_run(
    file_hunks: &diff_view::FileHunks,
    range: (usize, usize),
) -> Option<(usize, usize)> {
    let (range_start, range_end) = range;
    file_hunks
        .hunks
        .iter()
        .filter_map(|hunk| hunk.new_range)
        .filter(|&(hunk_start, hunk_end)| hunk_start <= hunk_end)
        .find_map(|(hunk_start, hunk_end)| {
            let start = hunk_start.max(range_start);
            let end = hunk_end.min(range_end);
            (start <= end).then_some((start, end))
        })
}
