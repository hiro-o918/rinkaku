//! "API changes" digest rendering (ADR 0036).
//!
//! A slim, contract-changes-only summary meant for the PR comment's
//! `<details>` section: one line per `Added`/`SignatureChanged`/removed
//! symbol, nothing else — no body-only changes, no dependency tree, no
//! full signatures for unclassified symbols. `render_markdown`'s full
//! report remains available as a separate action output for callers that
//! want it; this format exists specifically to be cheap for an LLM review
//! pass to read alongside the mermaid graph (ADR 0021/0035).

use crate::extract::{Classification, ExtractedSymbol, RemovedSymbol};
use crate::render::report::Report;
use std::fmt::Write as _;

/// Renders a [`Report`]'s contract-affecting symbols (ADR 0014's
/// classifications) as a flat Markdown list under an `### API changes`
/// heading. Walks `report.files` in source order (not the DFS order
/// `render_markdown`'s "Change graph" uses — a flat digest has no
/// tree/call-order structure for that distinction to convey), then
/// appends `report.removed` after every file's contract changes.
///
/// Returns an empty string when there is nothing to report (no
/// `Added`/`SignatureChanged` symbol and no removed symbol) — the caller
/// decides whether an empty digest still gets a heading/`<details>`
/// wrapper, the same convention `render_markdown` uses for its own
/// all-empty case.
///
/// Infallible for the same reason `render_mermaid` is: an owned `String`
/// buffer built via `push_str`/`write!` and only ever handed back to the
/// caller cannot fail the way an `io::Write` sink could.
pub(super) fn render_digest(report: &Report) -> String {
    let mut lines: Vec<String> = Vec::new();

    for file in &report.files {
        for symbol in &file.symbols {
            if let Some(line) = digest_line(symbol) {
                lines.push(line);
            }
        }
    }
    for removed in &report.removed {
        lines.push(removed_line(removed));
    }

    if lines.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("### API changes\n\n");
    for line in &lines {
        out.push_str(line);
    }
    out
}

/// Builds one digest entry for `symbol`, or `None` when its
/// classification isn't a contract change (`BodyOnly`, or classification
/// not attempted at all — always the case for a whole-repo outline, which
/// has no base side to classify against).
fn digest_line(symbol: &ExtractedSymbol) -> Option<String> {
    match symbol.classification {
        Some(Classification::Added) => Some(added_line(symbol)),
        Some(Classification::SignatureChanged) => Some(signature_changed_line(symbol)),
        Some(Classification::BodyOnly) | None => None,
    }
}

/// `+ name` header followed by the signature in a fenced code span — the
/// `+` mirrors the diff convention `signature_changed_line` uses below,
/// read as "added" without a "previous" side to diff against.
fn added_line(symbol: &ExtractedSymbol) -> String {
    let fence = fence_for(&symbol.signature);
    let mut out = String::new();
    writeln!(out, "- **+ {}**", symbol.name).expect("writing to a String cannot fail");
    writeln!(out, "  {fence}{}{fence}", symbol.signature).expect("writing to a String cannot fail");
    out
}

/// Symbol name header followed by a ` ```diff ` block (`-` previous
/// signature, `+` current signature) — the exact convention
/// `render_markdown`'s "Definitions" section already uses for
/// `SignatureChanged`, reused here so a reader who has seen either output
/// recognizes it immediately.
///
/// `previous_signature` is expected to be `Some` whenever `classification`
/// is `SignatureChanged` (`extract::classify_symbols`' invariant); falls
/// back to the plain fenced signature with no diff if not, rather than
/// panicking on a malformed report.
fn signature_changed_line(symbol: &ExtractedSymbol) -> String {
    let mut out = String::new();
    writeln!(out, "- **{}**", symbol.name).expect("writing to a String cannot fail");
    match &symbol.previous_signature {
        Some(previous_signature) => {
            let fence = fence_for_diff(previous_signature, &symbol.signature);
            writeln!(out, "  {fence}diff").expect("writing to a String cannot fail");
            writeln!(out, "  -{previous_signature}").expect("writing to a String cannot fail");
            writeln!(out, "  +{}", symbol.signature).expect("writing to a String cannot fail");
            writeln!(out, "  {fence}").expect("writing to a String cannot fail");
        }
        None => {
            let fence = fence_for(&symbol.signature);
            writeln!(out, "  {fence}{}{fence}", symbol.signature)
                .expect("writing to a String cannot fail");
        }
    }
    out
}

/// `~~name~~ — removed` — GitHub-native Markdown strikethrough, since a
/// removed symbol has no signature left to show at all (same data gap ADR
/// 0035 hit for the mermaid case).
fn removed_line(removed: &RemovedSymbol) -> String {
    format!("- ~~{}~~ — removed\n", removed.name)
}

/// Backtick fence wide enough to safely wrap `signature` inline (an inline
/// code span, not a fenced block — the digest keeps each entry to one or
/// two lines) — widens past the longest run of consecutive backticks
/// already in the text, the same defensive sizing `render_markdown`'s
/// `fence_for` uses, so a signature that happens to contain backticks
/// (unusual, but not impossible depending on the source language) can't
/// prematurely close the span.
fn fence_for(text: &str) -> String {
    "`".repeat((longest_backtick_run(text).unwrap_or(0) + 1).max(1))
}

/// [`fence_for`]'s sibling for a ` ```diff ` fenced block: widens against
/// both the previous and current signature text, mirroring
/// `render_markdown`'s `fence_for_diff`.
fn fence_for_diff(previous_signature: &str, signature: &str) -> String {
    let longest_run = [previous_signature, signature]
        .iter()
        .flat_map(|text| longest_backtick_run(text))
        .max()
        .unwrap_or(0);
    "`".repeat((longest_run + 1).max(3))
}

/// Length of the longest run of consecutive `` ` `` characters in `text`.
fn longest_backtick_run(text: &str) -> Option<usize> {
    text.split(|c| c != '`')
        .map(str::len)
        .filter(|&len| len > 0)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::LineRange;
    use crate::extract::SymbolKind;
    use crate::graph::SymbolGraph;
    use crate::render::report::{FileReport, ReportOrigin};
    use crate::render::{OutputFormat, render};
    use pretty_assertions::assert_eq;

    fn symbol(
        name: &str,
        signature: &str,
        classification: Option<Classification>,
        previous_signature: Option<&str>,
    ) -> ExtractedSymbol {
        ExtractedSymbol {
            id: format!("src/lib.rs::{name}"),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: signature.to_string(),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification,
            previous_signature: previous_signature.map(str::to_string),
        }
    }

    fn empty_report(files: Vec<FileReport>, removed: Vec<RemovedSymbol>) -> Report {
        Report {
            origin: ReportOrigin::Diff,
            files,
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
                roots: vec![],
            },
            tests: vec![],
            fan_ins: vec![],
            file_size_warnings: vec![],
            removed,
        }
    }

    #[test]
    fn should_render_empty_string_when_report_has_no_contract_changes() {
        let report = empty_report(
            vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol("untouched", "fn untouched()", None, None)],
            }],
            vec![],
        );

        let expected = String::new();
        let actual = render(&report, OutputFormat::Digest).expect("digest render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_omit_body_only_symbol_when_rendering_digest() {
        let report = empty_report(
            vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![
                    symbol(
                        "new_helper",
                        "fn new_helper()",
                        Some(Classification::Added),
                        None,
                    ),
                    symbol(
                        "tweaked_body",
                        "fn tweaked_body()",
                        Some(Classification::BodyOnly),
                        None,
                    ),
                ],
            }],
            vec![],
        );

        let expected = "\
### API changes

- **+ new_helper**
  `fn new_helper()`
"
        .to_string();
        let actual = render(&report, OutputFormat::Digest).expect("digest render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_diff_block_when_symbol_signature_changed() {
        let report = empty_report(
            vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "foo",
                    "fn foo(a: i32, b: i32) -> i32",
                    Some(Classification::SignatureChanged),
                    Some("fn foo(a: i32) -> i32"),
                )],
            }],
            vec![],
        );

        let expected = "\
### API changes

- **foo**
  ```diff
  -fn foo(a: i32) -> i32
  +fn foo(a: i32, b: i32) -> i32
  ```
"
        .to_string();
        let actual = render(&report, OutputFormat::Digest).expect("digest render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_strikethrough_when_symbol_was_removed() {
        let report = empty_report(
            vec![],
            vec![RemovedSymbol {
                name: "old_helper".to_string(),
                kind: SymbolKind::Function,
                path: "src/lib.rs".to_string(),
                signature: "fn old_helper()".to_string(),
            }],
        );

        let expected = "\
### API changes

- ~~old_helper~~ — removed
"
        .to_string();
        let actual = render(&report, OutputFormat::Digest).expect("digest render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_render_added_changed_and_removed_together_in_file_then_removed_order() {
        // Two files (a.rs: Added, b.rs: SignatureChanged), then one
        // removed symbol from a.rs — pins that ordering is per-file
        // source order for graph-backed symbols, with every removed
        // symbol appended after, regardless of which file it belonged to.
        let report = empty_report(
            vec![
                FileReport {
                    path: "src/a.rs".to_string(),
                    symbols: vec![symbol(
                        "new_in_a",
                        "fn new_in_a()",
                        Some(Classification::Added),
                        None,
                    )],
                },
                FileReport {
                    path: "src/b.rs".to_string(),
                    symbols: vec![symbol(
                        "changed_in_b",
                        "fn changed_in_b(x: i32)",
                        Some(Classification::SignatureChanged),
                        Some("fn changed_in_b()"),
                    )],
                },
            ],
            vec![RemovedSymbol {
                name: "removed_from_a".to_string(),
                kind: SymbolKind::Function,
                path: "src/a.rs".to_string(),
                signature: "fn removed_from_a()".to_string(),
            }],
        );

        let expected = "\
### API changes

- **+ new_in_a**
  `fn new_in_a()`
- **changed_in_b**
  ```diff
  -fn changed_in_b()
  +fn changed_in_b(x: i32)
  ```
- ~~removed_from_a~~ — removed
"
        .to_string();
        let actual = render(&report, OutputFormat::Digest).expect("digest render succeeds");

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_widen_diff_fence_when_signature_contains_a_backtick_run() {
        // A signature containing a triple-backtick run would prematurely
        // close a plain ```diff fence; fence_for_diff must widen past it.
        let report = empty_report(
            vec![FileReport {
                path: "src/lib.rs".to_string(),
                symbols: vec![symbol(
                    "weird",
                    "fn weird(s: &str) // ```embedded```",
                    Some(Classification::SignatureChanged),
                    Some("fn weird()"),
                )],
            }],
            vec![],
        );

        let expected = "\
### API changes

- **weird**
  ````diff
  -fn weird()
  +fn weird(s: &str) // ```embedded```
  ````
"
        .to_string();
        let actual = render(&report, OutputFormat::Digest).expect("digest render succeeds");

        assert_eq!(expected, actual);
    }
}
