//! Pluggable per-language support.
//!
//! `LanguageSupport` is the port through which the extraction pipeline
//! (`extract.rs`) reaches into a concrete tree-sitter grammar. It is kept
//! deliberately small: only the methods `extract.rs` (and, for
//! `reference_query`, `deps.rs`) actually call are declared here.

/// A language's tree-sitter-backed support: grammar plus the queries used to
/// locate definition nodes and the identifiers they reference.
pub trait LanguageSupport {
    /// Human-readable language name, e.g. `"rust"`.
    fn name(&self) -> &'static str;

    /// The tree-sitter grammar used to parse source files in this language.
    fn grammar(&self) -> tree_sitter::Language;

    /// Tree-sitter query that captures definition nodes (functions,
    /// structs, enums, traits, ...) whose signatures should be extracted.
    fn definition_query(&self) -> &str;

    /// Tree-sitter query that captures identifiers referenced from inside a
    /// definition: called function/method names (capture name starting
    /// with `reference.call`) and referenced type names (capture name
    /// starting with `reference.type`). `extract.rs`'s
    /// `collect_referenced_names` reads every capture under the
    /// `reference.` prefix, not a single outer capture — unlike
    /// `definition_query`, where the whole matched node is always the
    /// definition, a reference query's outer node (e.g. a whole
    /// `call_expression`) is not the identifier text callers want, only
    /// its `function`/`type` sub-capture is.
    ///
    /// Deliberately syntactic: local variables, parameter names, and
    /// built-in types (e.g. Rust `i32`, Go `string`, Python untyped names)
    /// are not filtered out explicitly — they are captured the same as any
    /// other identifier, but simply fail to resolve against the repo's
    /// definition index later, which has the same net effect without
    /// needing a per-language exclusion list.
    fn reference_query(&self) -> &str;

    /// Whether `path` is, by convention, a test file in its entirety (ADR
    /// 0009), e.g. Go's `*_test.go`, Python's `test_*.py`/`*_test.py`, or
    /// any language's `tests/`-directory convention. When `true`, every
    /// definition in the file is a test symbol regardless of
    /// [`is_test_definition`](LanguageSupport::is_test_definition) — the
    /// pipeline does not bother parsing the file to check node-level
    /// context in that case.
    fn is_test_path(&self, path: &str) -> bool;

    /// Whether `node` (a captured `@definition` node, or an ancestor of
    /// one) is a test symbol by its AST context rather than its file path
    /// — needed only for languages where test code is colocated with
    /// production code in the same file (Rust's `#[cfg(test)]` modules and
    /// `#[test]`/`#[rstest]`/`#[tokio::test]`-attributed functions).
    /// Defaults to `false`: most languages' test conventions are fully
    /// captured by [`is_test_path`](LanguageSupport::is_test_path), so
    /// there is nothing more to check at the node level.
    fn is_test_definition(&self, _node: tree_sitter::Node, _source: &[u8]) -> bool {
        false
    }

    /// Widens a captured `@definition` node's span to include any
    /// decorator/attribute annotating it (ADR 0073), so the touched-range
    /// check, `ExtractedSymbol::range`, and the extracted signature text
    /// all cover the decorator/attribute the same way they cover the
    /// definition itself. Defaults to `node` unchanged: Go, TypeScript, and
    /// HCL need no widening — TypeScript's grammar already nests a
    /// decorator inside `class_declaration`, and Go/HCL have no decorator
    /// syntax at all.
    fn definition_span_start<'a>(&self, node: tree_sitter::Node<'a>) -> tree_sitter::Node<'a> {
        node
    }
}

/// Looks up the `LanguageSupport` registered for a file path, based on
/// its path suffix. Returns `None` for unrecognized paths so callers
/// can skip files rinkaku doesn't understand yet, rather than erroring
/// out.
pub fn language_for_path(path: &str) -> Option<&'static dyn LanguageSupport> {
    REGISTRY
        .iter()
        .find(|lang| lang.suffixes().iter().any(|suffix| path.ends_with(suffix)))
        .map(|lang| lang.support())
}

/// One entry in the built-in language registry: the path suffixes that
/// route to a `LanguageSupport` impl. Suffixes rather than final
/// `.`-separated segments (ADR 0066): an entry can then name a
/// multi-segment convention (`.tftest.hcl`) without claiming every
/// file whose final segment merely coincides (plain `.hcl`). Entries
/// list more-specific suffixes first by convention; the first matching
/// entry wins.
struct RegistryEntry {
    suffixes: &'static [&'static str],
    support: fn() -> &'static dyn LanguageSupport,
}

impl RegistryEntry {
    fn suffixes(&self) -> &'static [&'static str] {
        self.suffixes
    }

    fn support(&self) -> &'static dyn LanguageSupport {
        (self.support)()
    }
}

/// Built-in languages, keyed by path suffix. Adding a language means
/// adding an entry here plus its `LanguageSupport` impl module — the
/// extraction pipeline itself does not change (ADR 0002).
///
/// `.js`/`.jsx` are intentionally out of scope for v1: the TypeScript
/// grammar only parses TypeScript syntax (type annotations etc.), and a
/// separate JavaScript grammar/`LanguageSupport` impl would be needed to
/// support plain JS files without misparsing or silently ignoring
/// TS-specific constructs. Revisit once there's a concrete need.
static REGISTRY: &[RegistryEntry] = &[
    RegistryEntry {
        suffixes: &[".rs"],
        support: || &rust::RustSupport,
    },
    RegistryEntry {
        suffixes: &[".go"],
        support: || &go::GoSupport,
    },
    RegistryEntry {
        suffixes: &[".py"],
        support: || &python::PythonSupport,
    },
    RegistryEntry {
        suffixes: &[".ts"],
        support: || &typescript::TypeScriptSupport,
    },
    RegistryEntry {
        suffixes: &[".tsx"],
        support: || &typescript::TsxSupport,
    },
    RegistryEntry {
        suffixes: &[".tf", ".tofu", ".tftest.hcl"],
        support: || &hcl::HclSupport,
    },
];

pub mod go;
pub mod hcl;
pub mod python;
pub mod rust;
pub mod typescript;

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn should_return_rust_support_when_path_has_rs_extension() {
        let actual = language_for_path("src/main.rs");

        let support = actual.expect("expected Some(&dyn LanguageSupport) for .rs path");
        assert_eq!("rust", support.name());
    }

    #[test]
    fn should_return_go_support_when_path_has_go_extension() {
        let actual = language_for_path("src/main.go");

        let support = actual.expect("expected Some(&dyn LanguageSupport) for .go path");
        assert_eq!("go", support.name());
    }

    #[test]
    fn should_return_python_support_when_path_has_py_extension() {
        let actual = language_for_path("src/main.py");

        let support = actual.expect("expected Some(&dyn LanguageSupport) for .py path");
        assert_eq!("python", support.name());
    }

    #[test]
    fn should_return_typescript_support_when_path_has_ts_extension() {
        let actual = language_for_path("src/main.ts");

        let support = actual.expect("expected Some(&dyn LanguageSupport) for .ts path");
        assert_eq!("typescript", support.name());
    }

    #[test]
    fn should_return_tsx_support_when_path_has_tsx_extension() {
        let actual = language_for_path("src/Component.tsx");

        let support = actual.expect("expected Some(&dyn LanguageSupport) for .tsx path");
        assert_eq!("typescript", support.name());
    }

    #[test]
    fn should_return_none_when_extension_is_unknown() {
        let actual = language_for_path("src/main.xyz");

        assert!(actual.is_none());
    }

    #[test]
    fn should_return_none_when_path_has_no_extension() {
        let actual = language_for_path("Makefile");

        assert!(actual.is_none());
    }

    #[rstest]
    #[case::should_return_none_when_filename_is_bare_rs("rs")]
    #[case::should_return_none_when_filename_is_bare_go("go")]
    #[case::should_return_none_when_filename_is_bare_py("py")]
    #[case::should_return_none_when_filename_is_bare_ts("ts")]
    #[case::should_return_none_when_filename_is_bare_tsx("tsx")]
    fn bare_extension_filenames_do_not_route(#[case] path: &str) {
        let actual = language_for_path(path);

        assert!(actual.is_none());
    }

    #[rstest]
    #[case::should_return_hcl_support_when_path_has_tf_suffix("envs/prod/main.tf")]
    #[case::should_return_hcl_support_when_path_has_tofu_suffix("envs/prod/main.tofu")]
    #[case::should_return_hcl_support_when_path_has_tftest_hcl_suffix("tests/plan.tftest.hcl")]
    fn hcl_paths_route_to_hcl_support(#[case] path: &str) {
        let actual = language_for_path(path);

        let support = actual.expect("expected Some(&dyn LanguageSupport) for a Terraform path");
        assert_eq!("hcl", support.name());
    }

    #[test]
    fn should_return_none_when_path_is_plain_hcl_dialect() {
        let actual = language_for_path("nomad/job.hcl");

        assert!(actual.is_none());
    }
}
