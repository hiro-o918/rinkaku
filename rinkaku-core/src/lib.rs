//! Core library for rinkaku.
//!
//! This crate will host the pure diff-condensation logic: parsing unified
//! diffs, locating changed symbol definitions via tree-sitter, and slicing
//! out signatures plus their 1-hop dependencies. IO (reading stdin, running
//! `git diff`, invoking LSP servers) stays at the boundary in `main.rs` and
//! future adapter modules, never inside this pure core.
//!
//! Placeholder function below will be removed once the first real feature
//! (diff parsing) lands; it only exists to prove the test harness works.

/// Adds two integers.
///
/// Placeholder pure function used to validate the test harness
/// (rstest + pretty_assertions) during project bootstrap.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_return_sum_when_both_inputs_are_positive(2, 3, 5)]
    #[case::should_return_zero_when_inputs_are_additive_inverses(5, -5, 0)]
    fn add_returns_expected_sum(#[case] a: i32, #[case] b: i32, #[case] expected: i32) {
        let actual = add(a, b);
        assert_eq!(expected, actual);
    }
}
