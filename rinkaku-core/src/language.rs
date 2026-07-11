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
}

/// Looks up the `LanguageSupport` registered for a file path, based on its
/// extension. Returns `None` for unrecognized extensions so callers can
/// skip files rinkaku doesn't understand yet, rather than erroring out.
pub fn language_for_path(path: &str) -> Option<&'static dyn LanguageSupport> {
    let extension = path.rsplit('.').next()?;
    REGISTRY
        .iter()
        .find(|lang| lang.extensions().contains(&extension))
        .map(|lang| lang.support())
}

/// One entry in the built-in language registry: the file extensions that
/// route to a `LanguageSupport` impl.
struct RegistryEntry {
    extensions: &'static [&'static str],
    support: fn() -> &'static dyn LanguageSupport,
}

impl RegistryEntry {
    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn support(&self) -> &'static dyn LanguageSupport {
        (self.support)()
    }
}

/// Built-in languages, keyed by file extension. Adding a language means
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
        extensions: &["rs"],
        support: || &rust::RustSupport,
    },
    RegistryEntry {
        extensions: &["go"],
        support: || &go::GoSupport,
    },
    RegistryEntry {
        extensions: &["py"],
        support: || &python::PythonSupport,
    },
    RegistryEntry {
        extensions: &["ts"],
        support: || &typescript::TypeScriptSupport,
    },
    RegistryEntry {
        extensions: &["tsx"],
        support: || &typescript::TsxSupport,
    },
];

pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

#[cfg(test)]
mod tests {
    use super::*;

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
}
