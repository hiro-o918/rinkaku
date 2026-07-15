//! Vim-like search in the Source view (ADR 0057): a pure state machine
//! (`SearchState`) for composing a query and stepping through its matches,
//! decoupled from `crate::ui` the same way `crate::review::ReviewState`
//! (ADR 0048) is decoupled from the rest of the TUI — `App` holds exactly
//! one field of it, every transition lives on `SearchState` itself.
//!
//! Match computation (`find_matches`/`smartcase_matches_line`) and
//! navigation (`next_match_index`/`prev_match_index`) are free functions
//! taking plain data in and returning plain data out, so they are
//! unit-testable without constructing an `App` or a `SearchState` at all.

/// Whether the query composing buffer is active, or a query has already
/// been confirmed (and its matches computed) — mirrors
/// [`crate::review::ReviewMode`]'s own "distinct phases of one feature"
/// shape.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SearchMode {
    #[default]
    Inactive,
    Composing {
        buffer: String,
    },
}

/// One matched line, 0-based, into the source view's `Vec<String>` of
/// lines — the same 0-based indexing [`crate::app::Screen::Source::scroll_top`]
/// already uses, so a match can be turned into a scroll target with no
/// coordinate conversion.
pub type MatchLine = usize;

/// The Source-view search feature's own state (ADR 0057's Module boundary
/// decision, following [`crate::review::ReviewState`]'s precedent): the
/// composing buffer or confirmed query, the confirmed query's computed
/// matches, and which one is current. Every method here is a pure state
/// transition — no IO, no `ratatui` types.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchState {
    mode: SearchMode,
    /// The last *confirmed* query — kept independent of `mode` so it
    /// survives `mode` returning to [`SearchMode::Inactive`] after
    /// confirming (unlike `mode`, which only holds a query while
    /// composing): `n`/`N` need the confirmed query's matches long after
    /// composing has ended, the same way vim's `/` search stays repeatable
    /// via `n`/`N` after the search prompt itself has closed.
    query: Option<String>,
    matches: Vec<MatchLine>,
    current: usize,
}

impl SearchState {
    pub fn mode(&self) -> &SearchMode {
        &self.mode
    }

    /// The last confirmed query, if any — `None` before the first
    /// confirmed search, or after [`Self::cancel`] clears it.
    pub fn query(&self) -> Option<&str> {
        self.query.as_deref()
    }

    pub fn matches(&self) -> &[MatchLine] {
        &self.matches
    }

    /// The confirmed query's current match count and 1-based position —
    /// `None` when there is no confirmed query at all, `Some((0, 0))` for a
    /// confirmed query with zero matches (the status line's own "no
    /// matches" case, ADR 0057 decision 7 — distinct from `None`, which
    /// means "not searching", not "searched and found nothing").
    pub fn match_position(&self) -> Option<(usize, usize)> {
        self.query.as_ref()?;
        if self.matches.is_empty() {
            return Some((0, 0));
        }
        Some((self.current + 1, self.matches.len()))
    }

    /// The current match's line, if any.
    pub fn current_match(&self) -> Option<MatchLine> {
        self.matches.get(self.current).copied()
    }

    /// Starts composing a new query — a no-op if already composing (Enter
    /// confirms and Esc cancels are the only ways out of that state, `/`
    /// pressed again mid-compose does not restart the buffer).
    pub fn start(mut self) -> Self {
        if matches!(self.mode, SearchMode::Inactive) {
            self.mode = SearchMode::Composing {
                buffer: String::new(),
            };
        }
        self
    }

    /// Appends `c` to the composing buffer — a no-op outside
    /// [`SearchMode::Composing`].
    pub fn push_char(mut self, c: char) -> Self {
        if let SearchMode::Composing { buffer } = &mut self.mode {
            buffer.push(c);
        }
        self
    }

    /// Removes the last character from the composing buffer — a no-op
    /// outside [`SearchMode::Composing`] or on an already-empty buffer.
    pub fn backspace(mut self) -> Self {
        if let SearchMode::Composing { buffer } = &mut self.mode {
            buffer.pop();
        }
        self
    }

    /// Confirms the composing buffer against `lines`, computing its matches
    /// and jumping to the first match at or after `from_line` (wrapping to
    /// the first match overall if none are at or after it) — a no-op
    /// outside [`SearchMode::Composing`]. An empty (or whitespace-only)
    /// buffer cancels instead of confirming an empty query, mirroring
    /// [`crate::review::ReviewState::confirm_compose`]'s identical "blank
    /// buffer means abandon, not commit" rule.
    pub fn confirm(mut self, lines: &[String], from_line: MatchLine) -> Self {
        let SearchMode::Composing { buffer } = &self.mode else {
            return self;
        };
        if buffer.trim().is_empty() {
            return self.cancel();
        }
        let query = buffer.clone();
        let matches = find_matches(lines, &query);
        let current = matches
            .iter()
            .position(|&line| line >= from_line)
            .unwrap_or(0);
        self.query = Some(query);
        self.matches = matches;
        self.current = current;
        self.mode = SearchMode::Inactive;
        self
    }

    /// Cancels composing (or clears an already-confirmed search) and
    /// discards every trace of it — ADR 0057 decision 2: cancel means "stop
    /// searching altogether", not "revert to the last confirmed query".
    pub fn cancel(mut self) -> Self {
        self.mode = SearchMode::Inactive;
        self.query = None;
        self.matches.clear();
        self.current = 0;
        self
    }

    /// Jumps to the next match, wrapping to the first — a no-op with no
    /// confirmed matches.
    pub fn next(mut self) -> Self {
        if let Some(index) = next_match_index(self.matches.len(), self.current) {
            self.current = index;
        }
        self
    }

    /// Jumps to the previous match, wrapping to the last — a no-op with no
    /// confirmed matches.
    pub fn prev(mut self) -> Self {
        if let Some(index) = prev_match_index(self.matches.len(), self.current) {
            self.current = index;
        }
        self
    }
}

/// Whether `query` should be matched case-sensitively under the smartcase
/// rule (ADR 0057 decision 5, the vim/ripgrep convention): a query
/// containing any uppercase character is case-sensitive; an all-lowercase
/// query (including one with no letters at all) is case-insensitive.
pub fn is_case_sensitive(query: &str) -> bool {
    query.chars().any(char::is_uppercase)
}

/// Whether `line` contains `query` as a literal substring under the
/// smartcase rule — the single-line primitive [`find_matches`] applies to
/// every line.
pub fn smartcase_matches_line(line: &str, query: &str) -> bool {
    if is_case_sensitive(query) {
        line.contains(query)
    } else {
        line.to_lowercase().contains(&query.to_lowercase())
    }
}

/// Every 0-based line index in `lines` containing `query` as a literal
/// smartcase substring, in ascending order — an empty `query` matches
/// nothing (mirrors [`SearchState::confirm`]'s own "blank buffer cancels"
/// rule: an empty query reaching this function at all would otherwise
/// match every line, which is never a useful search result).
pub fn find_matches(lines: &[String], query: &str) -> Vec<MatchLine> {
    if query.is_empty() {
        return Vec::new();
    }
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| smartcase_matches_line(line, query))
        .map(|(index, _)| index)
        .collect()
}

/// The next match index into a `total`-length match list after `current`,
/// wrapping to `0` — `None` when `total == 0` (nothing to navigate).
pub fn next_match_index(total: usize, current: usize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    Some((current + 1) % total)
}

/// The previous match index into a `total`-length match list before
/// `current`, wrapping to `total - 1` — `None` when `total == 0`.
pub fn prev_match_index(total: usize, current: usize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    Some((current + total - 1) % total)
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
