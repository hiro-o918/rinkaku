//! Widens a captured `@definition` node's span to include any
//! decorator/attribute annotating it (ADR 0073).
//!
//! [`LanguageSupport::definition_span_start`] locates where a definition's
//! span should actually begin (a Python `decorated_definition` wrapper, the
//! earliest of a run of Rust `attribute_item` siblings, or the node itself
//! for languages with no such wrapping). This module computes that once per
//! captured node — right after the query match, in
//! [`super::with_definition_nodes`] — and carries the result alongside the
//! original node so every downstream consumer (touched-range checks,
//! `ExtractedSymbol::range`, signature slicing, untouched-member removal)
//! reads the same widened span instead of each re-deriving it.

use super::LineRange;
use crate::language::LanguageSupport;

/// A captured `@definition` node plus the byte/row position its span
/// actually starts at once any decorator/attribute is folded in. `node`
/// itself is kept (not just the span start) because most tree-sitter
/// operations — walking children, finding the `body` field, and so on —
/// still need the original definition node, not its widened span; AST
/// nesting checks ([`super::is_descendant_of`]) also deliberately use
/// `node` rather than the span, since a decorator/attribute is never
/// itself an ancestor/descendant of anything.
#[derive(Clone, Copy)]
pub(super) struct DefinitionNode<'a> {
    pub(super) node: tree_sitter::Node<'a>,
    span_start_byte: usize,
    span_start_row: usize,
    span_start_column: usize,
}

impl<'a> DefinitionNode<'a> {
    pub(super) fn new(node: tree_sitter::Node<'a>, lang: &dyn LanguageSupport) -> Self {
        let span_start = lang.definition_span_start(node);
        Self {
            node,
            span_start_byte: span_start.start_byte(),
            span_start_row: span_start.start_position().row,
            span_start_column: span_start.start_position().column,
        }
    }

    pub(super) fn span_start_byte(&self) -> usize {
        self.span_start_byte
    }

    /// The widened span's starting column — the dedent baseline
    /// `tidy_signature_lines` needs (see `first_line_column`'s doc comment
    /// there), taken from the decorator/attribute's own indentation when
    /// present rather than the inner definition's, since the decorator line
    /// is now the signature's actual first line.
    pub(super) fn span_start_column(&self) -> usize {
        self.span_start_column
    }

    /// The widened span as a 1-based inclusive [`LineRange`]: starts at the
    /// decorator/attribute line when present, ends at `node`'s own last
    /// line (a decorator/attribute never extends the *end* of a
    /// definition, only its start).
    pub(super) fn line_range(&self) -> LineRange {
        LineRange {
            start: self.span_start_row + 1,
            end: self.node.end_position().row + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::python::PythonSupport;
    use crate::language::rust::RustSupport;
    use crate::language::typescript::TypeScriptSupport;
    use pretty_assertions::assert_eq;

    fn parse<'a>(
        parser: &'a mut tree_sitter::Parser,
        grammar: tree_sitter::Language,
        source: &'a str,
    ) -> tree_sitter::Tree {
        parser.set_language(&grammar).unwrap();
        parser.parse(source, None).unwrap()
    }

    #[test]
    fn should_widen_span_to_decorator_when_python_function_is_decorated() {
        let source = "@decorator\ndef foo(a):\n    return a\n";
        let mut parser = tree_sitter::Parser::new();
        let lang = PythonSupport;
        let tree = parse(&mut parser, lang.grammar(), source);
        let function_node = tree.root_node().named_child(0).unwrap();
        assert_eq!("decorated_definition", function_node.kind());
        let inner = function_node.named_child(1).unwrap();
        assert_eq!("function_definition", inner.kind());

        let actual = DefinitionNode::new(inner, &lang);

        assert_eq!(0, actual.span_start_byte());
        assert_eq!(LineRange { start: 1, end: 3 }, actual.line_range());
    }

    #[test]
    fn should_not_widen_span_when_python_function_is_undecorated() {
        let source = "def foo(a):\n    return a\n";
        let mut parser = tree_sitter::Parser::new();
        let lang = PythonSupport;
        let tree = parse(&mut parser, lang.grammar(), source);
        let function_node = tree.root_node().named_child(0).unwrap();
        assert_eq!("function_definition", function_node.kind());

        let actual = DefinitionNode::new(function_node, &lang);

        assert_eq!(function_node.start_byte(), actual.span_start_byte());
        assert_eq!(LineRange { start: 1, end: 2 }, actual.line_range());
    }

    #[test]
    fn should_widen_span_to_earliest_attribute_when_rust_item_has_stacked_attributes() {
        let source = "#[allow(dead_code)]\n#[derive(Debug)]\nstruct Foo {\n    x: i32,\n}\n";
        let mut parser = tree_sitter::Parser::new();
        let lang = RustSupport;
        let tree = parse(&mut parser, lang.grammar(), source);
        let struct_node = tree
            .root_node()
            .named_children(&mut tree.root_node().walk())
            .find(|n| n.kind() == "struct_item")
            .unwrap();

        let actual = DefinitionNode::new(struct_node, &lang);

        assert_eq!(0, actual.span_start_byte());
        assert_eq!(LineRange { start: 1, end: 5 }, actual.line_range());
    }

    #[test]
    fn should_not_widen_span_past_comment_when_rust_attribute_is_separated_by_comment() {
        let source = "// a comment\n#[derive(Debug)]\nstruct Foo {\n    x: i32,\n}\n";
        let mut parser = tree_sitter::Parser::new();
        let lang = RustSupport;
        let tree = parse(&mut parser, lang.grammar(), source);
        let struct_node = tree
            .root_node()
            .named_children(&mut tree.root_node().walk())
            .find(|n| n.kind() == "struct_item")
            .unwrap();

        let actual = DefinitionNode::new(struct_node, &lang);

        assert_eq!(LineRange { start: 2, end: 5 }, actual.line_range());
    }

    #[test]
    fn should_widen_span_to_decorator_when_typescript_exported_class_is_decorated() {
        let source = "@Component()\nexport class Widget {\n    label: string;\n}\n";
        let mut parser = tree_sitter::Parser::new();
        let lang = TypeScriptSupport;
        let tree = parse(&mut parser, lang.grammar(), source);
        let export_statement = tree.root_node().named_child(0).unwrap();
        assert_eq!("export_statement", export_statement.kind());
        let class_node = export_statement.child_by_field_name("declaration").unwrap();
        assert_eq!("class_declaration", class_node.kind());

        let actual = DefinitionNode::new(class_node, &lang);

        assert_eq!(0, actual.span_start_byte());
        assert_eq!(LineRange { start: 1, end: 4 }, actual.line_range());
    }

    #[test]
    fn should_not_widen_span_when_typescript_exported_class_is_undecorated() {
        let source = "export class Widget {\n    label: string;\n}\n";
        let mut parser = tree_sitter::Parser::new();
        let lang = TypeScriptSupport;
        let tree = parse(&mut parser, lang.grammar(), source);
        let export_statement = tree.root_node().named_child(0).unwrap();
        assert_eq!("export_statement", export_statement.kind());
        let class_node = export_statement.child_by_field_name("declaration").unwrap();
        assert_eq!("class_declaration", class_node.kind());

        let actual = DefinitionNode::new(class_node, &lang);

        assert_eq!(class_node.start_byte(), actual.span_start_byte());
        assert_eq!(LineRange { start: 1, end: 3 }, actual.line_range());
    }
}
