//! Rendering the extraction pipeline's results into an output format.
//!
//! [`Report`] is the pipeline-wide result shape produced by either
//! [`crate::pipeline::analyze_diff`] (a diff, [`ReportOrigin::Diff`]) or
//! [`crate::pipeline::analyze_repo`] (a whole-repo outline with no diff
//! involved, [`ReportOrigin::RepoOutline`] — ADR 0017): per-file extracted
//! symbols plus the files that were skipped (unsupported language, binary,
//! or deleted; `analyze_repo` never populates this), plus the
//! [`crate::graph::SymbolGraph`] built over those symbols (ADR 0008). This
//! module turns a `Report` into either Markdown (the default, meant for
//! humans and LLMs) or JSON (`serde`-derived, for machine consumption).
//!
//! Markdown renders in this order: a "Change graph" tree for a diff, or
//! "Repository graph" for a whole-repo outline (names only, rooted at the
//! graph's auto-detected entry points) giving the reader a call-hierarchy
//! reading order, with an optional "Hotspots" sub-section (ADR 0013) right
//! after it; "Definitions" — the full signature of every symbol, in the
//! same tree order, each shown exactly once (ADR 0008's decision to avoid
//! duplicating a symbol reachable from multiple roots); "Removed symbols" —
//! base-side symbols with no head-side counterpart at all (ADR 0014,
//! diff-only: `report.removed` is always empty for a whole-repo outline),
//! omitted when empty; "Tests" — a per-file count of changed test symbols
//! excluded from the graph/definitions above by default (ADR 0009); "Other
//! changed files" — files with no changed-symbol-level content (e.g. pure
//! renames); and "Skipped files". A whole-repo outline's wording drops every
//! "changed" qualifier (`report.origin` picks the noun — see
//! `change_graph_summary`), since nothing changed in that mode.
//!
//! ADR 0014 also marks each "Change graph"/"Hotspots"/"Definitions" line
//! with its contract-impact classification (`— new` / `— signature
//! changed`; `body_only` and not-attempted classifications render
//! unmarked), and a `signature_changed` symbol's "Definitions" entry shows
//! a ` ```diff ` block (base signature as `-`, head signature as `+`)
//! instead of the plain fenced signature every other classification gets.
//!
//! Skipped files are listed, never silently dropped, with one exception:
//! `SkipReason::Generated` entries are omitted from Markdown entirely (ADR
//! 0010/0011) — a `.gitattributes` declaration or a linguist-compatible
//! content marker has already told the repository this file is
//! uninteresting to diff-review, so listing it as something rinkaku
//! "didn't look at" would just be noise. Every other skip reason still
//! always appears, since a reviewer or LLM consuming the output needs to
//! know what rinkaku didn't look at. Test symbols are summarized rather
//! than dropped outright for the same reason: a reviewer still wants to
//! know "did this change come with tests?" even though the individual test
//! signatures are noise (ADR 0009).

mod markdown;
mod mermaid;
mod report;
mod shared;

pub use report::{
    FileReport, Report, ReportOrigin, SkipReason, SkippedFile, TestFileSummary, skip_reason_label,
};

use thiserror::Error;

/// Supported output formats for a [`Report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Markdown,
    Json,
    /// A human-oriented call/dependency graph rendered as a mermaid
    /// `flowchart` document (ADR 0021) — opt-in, aimed at GitHub's native
    /// mermaid rendering (PR comments/descriptions), not the default
    /// Markdown output ADR 0013/0015 keep machine-facing.
    Mermaid,
}

/// Errors that can occur while rendering a [`Report`].
#[derive(Debug, Error)]
pub enum RenderError {
    /// Writing to the in-memory `String` buffer failed. This only happens
    /// on allocation failure, which `std::fmt::Write` reports as `Err(())`
    /// with no further detail; kept as a typed error (rather than
    /// `.unwrap()`) so the fallible write calls in `render_markdown` can
    /// use `?` instead of panicking.
    #[error("failed to write Markdown output")]
    Fmt(#[from] std::fmt::Error),
    #[error("failed to serialize report as JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// Renders a [`Report`] in the requested [`OutputFormat`].
pub fn render(report: &Report, format: OutputFormat) -> Result<String, RenderError> {
    match format {
        OutputFormat::Markdown => markdown::render_markdown(report),
        OutputFormat::Json => Ok(serde_json::to_string_pretty(report)?),
        OutputFormat::Mermaid => Ok(mermaid::render_mermaid(report)),
    }
}
