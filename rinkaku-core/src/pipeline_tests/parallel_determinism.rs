//! ADR 0029 regression: [`analyze_repo`]'s per-file loop is now driven
//! by rayon's `par_iter`, whose ordered `collect` contract is what keeps
//! the output for a given input deterministic (byte-identical across
//! runs and, within a single run, in the same order as the input
//! `paths`). Locks that invariant down at the top-level `Report` so any
//! future accidental switch to an unordered combinator (e.g.
//! `par_bridge`, unordered `flat_map`, `fold`+`reduce` without a merge)
//! fails loudly here rather than only misbehaving on the
//! three-crate-workspace test set that happens to have short enough
//! inputs to hide it.

use super::fake_reader;
use crate::pipeline::analyze_repo;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[test]
fn should_produce_deterministic_output_on_repeated_calls() {
    // Ten distinct files with distinct symbol shapes across three
    // languages: enough distinct paths that a shuffled order would
    // show up in the `Vec<FileReport>`/graph node lists rather
    // than being masked by a same-content file being reordered
    // with itself.
    let files: Vec<(&'static str, &'static str)> = vec![
        ("src/a.rs", "fn a1() {}\nfn a2() {}\n"),
        ("src/b.rs", "fn b1() {}\nstruct B { x: i32 }\n"),
        ("src/c.rs", "fn c1() {}\ntrait C { fn m(&self); }\n"),
        ("src/d.rs", "fn d1() {}\nenum D { X, Y }\n"),
        ("src/e.rs", "fn e1(x: i32) -> i32 { x }\n"),
        ("pkg/f.go", "package pkg\n\nfunc F1() {}\nfunc F2() {}\n"),
        ("pkg/g.go", "package pkg\n\ntype G struct{}\n"),
        ("py/h.py", "def h1():\n    pass\n\ndef h2():\n    pass\n"),
        ("py/i.py", "class I:\n    def m(self):\n        pass\n"),
        (
            "py/j.py",
            "def j1(x):\n    return x\n\ndef j2(y):\n    return y\n",
        ),
    ];
    let read_file = fake_reader(HashMap::from_iter(files.iter().copied()));
    let paths: Vec<String> = files.iter().map(|(p, _)| p.to_string()).collect();

    let first = analyze_repo(&paths, &read_file, true, &HashSet::new(), true);
    // Repeated calls must produce byte-identical `Report`s: the
    // per-file body is pure (no interior mutability, no
    // wall-clock), rayon's `par_iter().collect()` preserves source
    // order, and downstream graph building is already
    // deterministic — so any inequality here means one of those
    // invariants regressed.
    for _ in 0..4 {
        let again = analyze_repo(&paths, &read_file, true, &HashSet::new(), true);
        assert_eq!(first, again);
    }

    // Source-order invariant: the `Vec<FileReport>` must be in
    // the same order as the input `paths` (rayon's ordered
    // `collect` contract). Every path here maps to one
    // `FileReport`, so equality of the two path lists is the
    // strongest possible check.
    let expected_paths: Vec<String> = paths.clone();
    let actual_paths: Vec<String> = first.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(expected_paths, actual_paths);
}
