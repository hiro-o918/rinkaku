use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_use_the_node_path_as_search_text_for_dir_and_file_rows() {
    let tree = sample_tree();
    let nav = Nav::new();
    let rows = nav.rows(&tree);

    let texts = row_search_texts(&rows);

    // sample_tree()'s expanded row order: src(0, Dir), src/lib.rs(1, File),
    // foo(2, Symbol), bar(3, Symbol).
    assert_eq!("src", texts[0]);
    assert_eq!("src/lib.rs", texts[1]);
}

#[test]
fn should_use_the_symbol_name_as_search_text_for_symbol_rows() {
    let tree = sample_tree();
    let nav = Nav::new();
    let rows = nav.rows(&tree);

    let texts = row_search_texts(&rows);

    assert_eq!("foo", texts[2]);
    assert_eq!("bar", texts[3]);
}

#[test]
fn should_return_one_text_per_row_in_the_same_order() {
    let tree = sample_tree();
    let nav = Nav::new();
    let rows = nav.rows(&tree);

    let texts = row_search_texts(&rows);

    assert_eq!(rows.len(), texts.len());
}
