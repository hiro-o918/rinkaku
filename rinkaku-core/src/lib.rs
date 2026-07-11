//! Core library for rinkaku.
//!
//! This crate will host the pure diff-condensation logic: parsing unified
//! diffs, locating changed symbol definitions via tree-sitter, and slicing
//! out signatures plus their 1-hop dependencies. IO (reading stdin, running
//! `git diff`, invoking LSP servers) stays at the boundary in `main.rs` and
//! future adapter modules, never inside this pure core.
