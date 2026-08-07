//! End-to-end pin for ADR 0064 (amended by #236, issue #230): a
//! stoplisted receiver call (`state.get()`) must create no graph edge,
//! while the same-named trait method spec (`fn get(&self) -> i32;`)
//! still feeds `referenced_method_names` and links to its implementation
//! — through the full [`analyze_diff`] pipeline, not just
//! `extract_changed_symbols` (already pinned at the extract level by
//! `extract_tests::rust_references::should_keep_stoplisted_trait_method_name_but_still_drop_it_from_a_receiver_call`).

use super::fake_reader;
use crate::pipeline::analyze_diff;
use std::collections::{HashMap, HashSet};

#[test]
fn should_link_trait_methodspec_to_impl_but_not_a_stoplisted_receiver_call_on_the_same_name() {
    let diff = "\
diff --git a/src/lib.rs b/src/lib.rs
index 57b03c6..1e5c9a1 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,15 +1,15 @@
-trait Cache {
+pub trait Cache {
     fn get(&self) -> i32;
 }

 struct Store;

 impl Cache for Store {
-    fn get(&self) -> i32 {
-        0
+    fn get(&self) -> i32 {
+        1
     }
 }

 fn build(state: Store) -> i32 {
-    state.get()
+    state.get() as i32
 }
";
    let source = "\
pub trait Cache {
    fn get(&self) -> i32;
}

struct Store;

impl Cache for Store {
    fn get(&self) -> i32 {
        1
    }
}

fn build(state: Store) -> i32 {
    state.get() as i32
}
";
    let read_file = fake_reader(HashMap::from([("src/lib.rs", source)]));

    let report = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    // The trait's own declaration line changed, so the whole trait —
    // not just its inner `get` method spec — is the extracted symbol
    // (the "narrowest enclosing definition" rule, same as
    // extract_tests::rust::should_extract_trait_signature_when_no_method_line_specifically_changed).
    let trait_node = report
        .graph
        .nodes
        .iter()
        .find(|n| n.name == "Cache")
        .expect("Cache trait node should exist");
    let impl_get_node = report
        .graph
        .nodes
        .iter()
        .find(|n| n.name == "get" && n.container.as_deref() == Some("impl Store"))
        .expect("impl Store's get node should exist");
    let build_node = report
        .graph
        .nodes
        .iter()
        .find(|n| n.name == "build")
        .expect("build node should exist");

    let has_edge = |from: &str, to: &str| {
        report
            .graph
            .edges
            .iter()
            .any(|e| e.from == from && e.to == to)
    };

    // The trait method spec's `get` still links to the impl's `get` —
    // ContainerRule::Any for referenced_method_names is unaffected by the
    // stoplist, which only scopes to receiver-call captures.
    assert!(
        has_edge(&trait_node.id, &impl_get_node.id),
        "expected an edge from the trait method spec to its impl"
    );
    // The stoplisted receiver call `state.get()` must not create an edge
    // from `build` to either `get` node.
    assert!(
        !has_edge(&build_node.id, &impl_get_node.id),
        "expected no edge from a stoplisted receiver call to the impl method"
    );
    assert!(
        !has_edge(&build_node.id, &trait_node.id),
        "expected no edge from a stoplisted receiver call to the trait"
    );
}
