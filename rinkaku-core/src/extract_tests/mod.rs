//! Test suite for [`crate::extract`], split by responsibility per ADR 0028
//! so the production file stays under the file-size warn threshold.
//!
//! Topic modules:
//!
//! - [`rust`] — Rust `LanguageSupport`: pins `extract_changed_symbols` /
//!   `extract_all_symbols` behavior on Rust sources (function/struct/enum/
//!   trait/impl containers, comment stripping, `#[cfg(test)]` / `#[test]`
//!   detection, noise-name filtering, and the shared Rust end-to-end
//!   pipeline path via `parse_unified_diff` + `language_for_path`).
//! - [`go`] — Go `LanguageSupport`: struct/interface/type_spec handling,
//!   pointer- vs. value-receiver container naming, and Go end-to-end.
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
mod python;
mod rust;
mod typescript;
