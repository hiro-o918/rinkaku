//! View-models for rinkaku's terminal UI (ADR 0015/0016).
//!
//! This crate holds only plain data and pure functions/state machines
//! derived from [`rinkaku_core::render::Report`] — directory-tree
//! building, topological ordering, navigation state, and the detail-pane
//! view-model. No `ratatui`/`crossterm` types appear in any public
//! signature here (ADR 0016 decision 3): rendering those view-models to
//! the terminal is a later stage's job, layered on top of this crate.
