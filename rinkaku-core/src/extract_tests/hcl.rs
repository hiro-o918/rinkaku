//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on HCL / Terraform sources:
//! top-level block naming, header- vs. whole-block signatures, `locals`
//! expansion, ignored and nested blocks, and the HCL end-to-end path via
//! `parse_unified_diff` + `language_for_path`.

use super::*;
use crate::language::hcl::HclSupport;
use pretty_assertions::assert_eq;

#[test]
fn should_extract_resource_header_signature_when_body_line_changed() {
    let source = "\
resource \"aws_instance\" \"web\" {
  ami           = \"ami-123\"
  instance_type = \"t3.micro\"
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "aws_instance.web".to_string(),
        kind: SymbolKind::Block,
        signature: "resource \"aws_instance\" \"web\"".to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_normalize_traversal_references_when_resource_body_changed() {
    let source = "\
resource \"aws_instance\" \"web\" {
  ami           = data.aws_ami.ubuntu.id
  instance_type = var.instance_type
  subnet_id     = module.vpc.public_subnet_id
  name          = local.name_prefix
  peer          = aws_instance.other.id
  count_alias   = count.index
  each_alias    = each.value
  self_alias    = self.public_ip
  mod_path      = path.module
  ws            = terraform.workspace
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 11 }];

    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    // Partial comparison: pins referenced-name normalization only; the
    // full symbol shape is pinned by
    // should_extract_resource_header_signature_when_body_line_changed.
    let expected_names = vec![
        "aws_instance.other".to_string(),
        "data.aws_ami.ubuntu".to_string(),
        "local.name_prefix".to_string(),
        "module.vpc".to_string(),
        "var.instance_type".to_string(),
    ];
    assert_eq!(expected_names, actual[0].referenced_names);
}

#[test]
fn should_expand_locals_block_into_per_attribute_symbols_when_one_attribute_changed() {
    let source = "\
locals {
  name_prefix = \"demo\"
  port        = 8080
}
";
    let lang = HclSupport;
    // Only line 2 (`name_prefix`) changed: the untouched sibling
    // attribute `port` must not be reported.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "local.name_prefix".to_string(),
        kind: SymbolKind::Block,
        signature: "name_prefix = \"demo\"".to_string(),
        range: LineRange { start: 2, end: 2 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_collect_traversals_inside_string_interpolation_when_local_value_changed() {
    let source = "\
locals {
  name = \"x-${var.env}\"
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    // Partial comparison: pins referenced-name normalization only; the
    // full `local.<attr>` symbol shape is pinned by
    // should_expand_locals_block_into_per_attribute_symbols_when_one_attribute_changed.
    let expected_names = vec!["var.env".to_string()];
    assert_eq!(expected_names, actual[0].referenced_names);
}

#[test]
fn should_extract_variable_whole_block_signature_when_default_line_changed() {
    let source = "\
variable \"region\" {
  type    = string
  default = \"us-east-1\"
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "var.region".to_string(),
        kind: SymbolKind::Block,
        signature: "variable \"region\" {\n  type    = string\n  default = \"us-east-1\"\n}"
            .to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_data_header_signature_when_body_line_changed() {
    let source = "\
data \"aws_ami\" \"ubuntu\" {
  most_recent = true
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "data.aws_ami.ubuntu".to_string(),
        kind: SymbolKind::Block,
        signature: "data \"aws_ami\" \"ubuntu\"".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_module_header_signature_when_body_line_changed() {
    let source = "\
module \"vpc\" {
  source = \"./modules/vpc\"
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "module.vpc".to_string(),
        kind: SymbolKind::Block,
        signature: "module \"vpc\"".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_output_whole_block_signature_when_value_line_changed() {
    let source = "\
output \"instance_ip\" {
  value       = \"192.0.2.1\"
  description = \"Public IP\"
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "output.instance_ip".to_string(),
        kind: SymbolKind::Block,
        signature:
            "output \"instance_ip\" {\n  value       = \"192.0.2.1\"\n  description = \"Public IP\"\n}"
                .to_string(),
        range: LineRange { start: 1, end: 4 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_provider_header_signature_when_body_line_changed() {
    let source = "\
provider \"aws\" {
  region = \"us-east-1\"
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "provider.aws".to_string(),
        kind: SymbolKind::Block,
        signature: "provider \"aws\"".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_report_terraform_or_unknown_blocks_when_changed() {
    let source = "\
terraform {
  required_version = \">= 1.9\"
}

moved {
  from = \"old\"
  to   = \"new\"
}

import {
}

check \"x\" {
}
";
    let lang = HclSupport;
    let changed_ranges = vec![
        LineRange { start: 2, end: 2 },
        LineRange { start: 6, end: 7 },
        LineRange { start: 10, end: 11 },
        LineRange { start: 13, end: 14 },
    ];

    let expected: Vec<ExtractedSymbol> = vec![];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_not_report_nested_block_as_symbol_when_only_it_changed() {
    let source = "\
resource \"aws_instance\" \"web\" {
  ami = \"ami-123\"
  lifecycle {
    create_before_destroy = true
  }
}
";
    let lang = HclSupport;
    let changed_ranges = vec![LineRange { start: 4, end: 4 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "aws_instance.web".to_string(),
        kind: SymbolKind::Block,
        signature: "resource \"aws_instance\" \"web\"".to_string(),
        range: LineRange { start: 1, end: 6 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_all_symbols_when_indexing_hcl_file() {
    let source = "\
variable \"region\" {
  type    = string
  default = \"us-east-1\"
}

resource \"aws_instance\" \"web\" {
  ami = \"ami-123\"
}

locals {
  name_prefix = \"demo\"
  port        = 8080
}
";
    let lang = HclSupport;

    let expected = vec![
        ExtractedSymbol {
            id: String::new(),
            name: "var.region".to_string(),
            kind: SymbolKind::Block,
            signature: "variable \"region\" {\n  type    = string\n  default = \"us-east-1\"\n}"
                .to_string(),
            range: LineRange { start: 1, end: 4 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
        ExtractedSymbol {
            id: String::new(),
            name: "aws_instance.web".to_string(),
            kind: SymbolKind::Block,
            signature: "resource \"aws_instance\" \"web\"".to_string(),
            range: LineRange { start: 6, end: 8 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
        ExtractedSymbol {
            id: String::new(),
            name: "local.name_prefix".to_string(),
            kind: SymbolKind::Block,
            signature: "name_prefix = \"demo\"".to_string(),
            range: LineRange { start: 11, end: 11 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
        ExtractedSymbol {
            id: String::new(),
            name: "local.port".to_string(),
            kind: SymbolKind::Block,
            signature: "port        = 8080".to_string(),
            range: LineRange { start: 12, end: 12 },
            container: None,
            referenced_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        },
    ];
    let actual = extract_all_symbols(source, &lang);

    assert_eq!(expected, actual);
}

#[test]
fn should_extract_hcl_symbols_when_diff_routed_by_suffix() {
    use crate::diff::parse_unified_diff;
    use crate::language::language_for_path;

    let diff = "\
diff --git a/main.tf b/main.tf
index e69de29..4b825dc 100644
--- a/main.tf
+++ b/main.tf
@@ -1,3 +1,3 @@
 resource \"aws_instance\" \"web\" {
-  ami = \"ami-old\"
+  ami = \"ami-new\"
 }
";
    let source = "\
resource \"aws_instance\" \"web\" {
  ami = \"ami-new\"
}
";
    let changed_file = parse_unified_diff(diff)
        .expect("diff should parse")
        .into_iter()
        .next()
        .expect("diff should contain one changed file");
    let lang = language_for_path(&changed_file.path).expect("*.tf should resolve to HCL");

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "aws_instance.web".to_string(),
        kind: SymbolKind::Block,
        signature: "resource \"aws_instance\" \"web\"".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

    assert_eq!(expected, actual);
}
