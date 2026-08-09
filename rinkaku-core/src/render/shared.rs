//! Cross-format render helpers.
//!
//! [`SymbolLookup`] indexes a `Report`'s per-file symbols by `NodeId` so
//! Markdown and mermaid rendering can go from a graph node back to the
//! full [`ExtractedSymbol`] (signature, container, dependencies) it
//! represents without each format re-walking `Report.files` on every
//! lookup.

use crate::extract::ExtractedSymbol;
use crate::render::report::FileReport;
use std::collections::HashMap;

/// A changed symbol paired with the path of the file it lives in, keyed by
/// [`crate::graph::NodeId`] — the lookup table rendering needs to go from a
/// graph node back to the full [`ExtractedSymbol`] (signature, container,
/// dependencies) it represents.
pub(super) struct SymbolLookup<'a> {
    by_id: HashMap<&'a str, (&'a str, &'a ExtractedSymbol)>,
}

impl<'a> SymbolLookup<'a> {
    pub(super) fn build(files: &'a [FileReport]) -> Self {
        let symbol_count = files.iter().map(|file| file.symbols.len()).sum();
        let mut by_id = HashMap::with_capacity(symbol_count);
        for file in files {
            for symbol in &file.symbols {
                by_id.insert(symbol.id.as_str(), (file.path.as_str(), symbol));
            }
        }
        Self { by_id }
    }

    pub(super) fn get(&self, id: &str) -> Option<(&'a str, &'a ExtractedSymbol)> {
        self.by_id.get(id).copied()
    }
}

pub(super) fn backtick_fence<'a>(
    contents: impl IntoIterator<Item = &'a str>,
    minimum_length: usize,
) -> String {
    let longest_run = contents
        .into_iter()
        .flat_map(longest_backtick_run)
        .max()
        .unwrap_or(0);
    "`".repeat((longest_run + 1).max(minimum_length))
}

fn longest_backtick_run(text: &str) -> Option<usize> {
    text.split(|character| character != '`')
        .map(str::len)
        .filter(|&length| length > 0)
        .max()
}
