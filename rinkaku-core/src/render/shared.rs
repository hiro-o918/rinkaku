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
        let mut by_id = HashMap::new();
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
