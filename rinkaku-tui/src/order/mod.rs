//! Entry-view ordering (ADR 0016 decision 4): [`rank`] computes each
//! directory's position in the production dependency order, [`sort`]
//! applies that order (or plain A-Z) to a [`crate::tree::Tree`].

mod rank;
mod sort;

pub use rank::{
    CycleEdge, DirRank, cycle_edges, cycle_explanation, cycle_partners, rank_directories,
};
pub use sort::{OrderMode, order_tree};
