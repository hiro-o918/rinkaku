//! Test suite for [`crate::extract`], split by responsibility per ADR 0028
//! so the production file stays under the file-size warn threshold.
//!
//! Topic modules:
//!
//! - [`rust`] — Rust `LanguageSupport`: pins `extract_changed_symbols` /
//!   `extract_all_symbols` behavior on Rust sources (function/struct/enum/
//!   trait/impl containers, comment stripping, `#[cfg(test)]` / `#[test]`
//!   detection, and the shared Rust end-to-end pipeline path via
//!   `parse_unified_diff` + `language_for_path`).
//! - [`rust_references`] — reference-name collection on Rust sources:
//!   noise-name filtering, scoped-path captures, macro-body walking
//!   (ADR 0063), and module-scoped / method-call captures with the
//!   ubiquitous-name stoplist (ADR 0064).
//! - [`go`] — Go `LanguageSupport`: struct/interface/type_spec handling,
//!   pointer- vs. value-receiver container naming, and Go end-to-end.
//! - [`hcl`] — HCL / Terraform `LanguageSupport`: top-level block naming,
//!   header- vs. whole-block signatures, `locals` expansion, and HCL
//!   end-to-end.
//! - [`python`] — Python `LanguageSupport`: class signature slicing with
//!   method bodies stripped, decorator/nested-function edge cases, and
//!   Python end-to-end.
//! - [`typescript`] — TypeScript / TSX `LanguageSupport`: interface, type
//!   alias, enum, arrow-function const bindings, abstract class/method
//!   signatures, class field arrow-function body stripping, and TS/TSX
//!   end-to-end.
//! - [`classification`] — `classify_symbols`: `Added` /
//!   `SignatureChanged` / `BodyOnly` classification and `RemovedSymbol`
//!   reporting (ADR 0014), matched by `(name, container)` identity.
//! - [`tidy_lines`] — `tidy_signature_lines`: dedenting, trailing-
//!   whitespace trimming, and blank-line collapsing applied to a
//!   multi-line signature slice (ADR 0060).
//! - [`normalize_for_comparison`] — the whitespace-run normalization
//!   `classify_symbols` uses to compare two signatures without false
//!   positives from a reflow-only edit (ADR 0060).

// Re-export `crate::extract`'s items so each topic submodule can pull them
// in with the customary `use super::*;`, mirroring what the original
// inline `mod tests { use super::*; }` block already had. Restricted to
// `pub(crate)` — needed because `pub(super) use super::*;` on a `use`
// item does not make those names visible to *this* module's children via
// their own `use super::*;` glob, whereas `pub(crate)` does.
#[allow(unused_imports)]
pub(crate) use super::*;

mod classification;
mod go;
mod hcl;
mod normalize_for_comparison;
mod python;
mod rust;
mod rust_references;
mod tidy_lines;
mod typescript;
