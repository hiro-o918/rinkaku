//! Regression: a Go receiver method changing alone — its struct entirely
//! untouched by the diff, unlike
//! `removed_container_regression`'s Python/TypeScript cases where the
//! container line range is present in the diff's hunk context — must
//! still be reported under its struct container, and the struct itself
//! must not be misreported as `removed` (ADR 0012 decision 2 for
//! container naming; PR #237's `all_head_symbols` fix for the removal
//! check).

use super::fake_reader;
use crate::extract::{Classification, RemovedSymbol};
use crate::pipeline::analyze_diff;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[test]
fn should_report_receiver_method_under_its_container_without_a_false_removed_struct() {
    let diff = "\
diff --git a/repo.go b/repo.go
index 57b03c6..1e5c9a1 100644
--- a/repo.go
+++ b/repo.go
@@ -8,5 +8,5 @@ type Repo struct {
 }

 func (r *Repo) Save(id string) error {
-\treturn errors.New(\"not implemented\")
+\treturn nil
 }
";
    let base_source = "\
package main

import \"errors\"

type Repo struct {
\tName string
}

func (r *Repo) Save(id string) error {
\treturn errors.New(\"not implemented\")
}
";
    let head_source = "\
package main

import \"errors\"

type Repo struct {
\tName string
}

func (r *Repo) Save(id string) error {
\treturn nil
}
";
    let read_file = fake_reader(HashMap::from([("repo.go", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("repo.go", base_source)]));

    let report = analyze_diff(
        diff,
        read_file,
        Some(&read_base_file),
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(1, report.files[0].symbols.len());
    let symbol = &report.files[0].symbols[0];
    assert_eq!("Save", symbol.name);
    assert_eq!(Some("Repo".to_string()), symbol.container);
    assert_eq!(Some(Classification::BodyOnly), symbol.classification);
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}
