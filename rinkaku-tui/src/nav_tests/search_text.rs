use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_use_the_node_path_for_dir_and_file_rows_and_the_symbol_name_for_symbol_rows() {
    let tree = sample_tree();
    let nav = Nav::new();
    let rows = nav.rows(&tree);

    let actual = row_search_texts(&rows);

    // sample_tree()'s expanded row order: src(0, Dir), src/lib.rs(1, File),
    // foo(2, Symbol), bar(3, Symbol).
    assert_eq!(
        vec![
            "src".to_string(),
            "src/lib.rs".to_string(),
            "foo".to_string(),
            "bar".to_string(),
        ],
        actual
    );
}
