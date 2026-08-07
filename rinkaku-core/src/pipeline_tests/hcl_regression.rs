//! HCL/Terraform end-to-end regression matrix via [`analyze_diff`]: a
//! deleted `resource` block is reported `removed`; a label rename shows
//! up as an `Added`/`removed` pair rather than a signature change (HCL
//! block identity is `(block_type, ...labels)`-based, per
//! [`crate::language::hcl`]); and editing one `locals` attribute leaves
//! its untouched sibling attributes out of both the changed-symbol list
//! and `removed`, mirroring `locals`' per-attribute expansion pinned at
//! the extract level in `extract_tests::hcl`.

use super::fake_reader;
use crate::extract::{Classification, RemovedSymbol, SymbolKind};
use crate::pipeline::analyze_diff;
use pretty_assertions::assert_eq;
use std::collections::{HashMap, HashSet};

#[test]
fn should_report_resource_as_removed_when_block_deleted_from_a_still_alive_file() {
    let diff = "\
diff --git a/main.tf b/main.tf
index 57b03c6..1e5c9a1 100644
--- a/main.tf
+++ b/main.tf
@@ -1,7 +1,3 @@
-resource \"aws_instance\" \"web\" {
-  ami           = \"ami-123\"
-  instance_type = \"t3.micro\"
-}
-
 resource \"aws_instance\" \"other\" {
   ami = \"ami-456\"
 }
";
    let base_source = "\
resource \"aws_instance\" \"web\" {
  ami           = \"ami-123\"
  instance_type = \"t3.micro\"
}

resource \"aws_instance\" \"other\" {
  ami = \"ami-456\"
}
";
    let head_source = "\
resource \"aws_instance\" \"other\" {
  ami = \"ami-456\"
}
";
    let read_file = fake_reader(HashMap::from([("main.tf", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("main.tf", base_source)]));

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

    let expected = vec![RemovedSymbol {
        name: "aws_instance.web".to_string(),
        kind: SymbolKind::Block,
        path: "main.tf".to_string(),
        signature: "resource \"aws_instance\" \"web\"".to_string(),
    }];
    assert_eq!(expected, report.removed);
}

// A label rename changes the block's identity (`(block_type, labels)`,
// per crate::language::hcl), so it is not a signature change on the same
// symbol — the old label must show up removed and the new label added,
// not merged into one SignatureChanged entry.
#[test]
fn should_report_added_and_removed_pair_when_resource_label_renamed() {
    let diff = "\
diff --git a/main.tf b/main.tf
index 57b03c6..1e5c9a1 100644
--- a/main.tf
+++ b/main.tf
@@ -1,3 +1,3 @@
-resource \"aws_instance\" \"web\" {
+resource \"aws_instance\" \"frontend\" {
   ami = \"ami-123\"
 }
";
    let base_source = "\
resource \"aws_instance\" \"web\" {
  ami = \"ami-123\"
}
";
    let head_source = "\
resource \"aws_instance\" \"frontend\" {
  ami = \"ami-123\"
}
";
    let read_file = fake_reader(HashMap::from([("main.tf", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("main.tf", base_source)]));

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
    assert_eq!("aws_instance.frontend", symbol.name);
    assert_eq!(Some(Classification::Added), symbol.classification);

    let expected_removed = vec![RemovedSymbol {
        name: "aws_instance.web".to_string(),
        kind: SymbolKind::Block,
        path: "main.tf".to_string(),
        signature: "resource \"aws_instance\" \"web\"".to_string(),
    }];
    assert_eq!(expected_removed, report.removed);
}

#[test]
fn should_report_only_the_changed_locals_attribute_when_a_sibling_attribute_is_untouched() {
    let diff = "\
diff --git a/main.tf b/main.tf
index 57b03c6..1e5c9a1 100644
--- a/main.tf
+++ b/main.tf
@@ -1,4 +1,4 @@
 locals {
-  name_prefix = \"demo\"
+  name_prefix = \"prod\"
   port        = 8080
 }
";
    let base_source = "\
locals {
  name_prefix = \"demo\"
  port        = 8080
}
";
    let head_source = "\
locals {
  name_prefix = \"prod\"
  port        = 8080
}
";
    let read_file = fake_reader(HashMap::from([("main.tf", head_source)]));
    let read_base_file = fake_reader(HashMap::from([("main.tf", base_source)]));

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
    assert_eq!("local.name_prefix", symbol.name);
    assert_eq!(
        Some(Classification::SignatureChanged),
        symbol.classification
    );
    assert_eq!(Vec::<RemovedSymbol>::new(), report.removed);
}
