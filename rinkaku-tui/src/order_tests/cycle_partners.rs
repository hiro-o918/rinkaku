use super::*;
use pretty_assertions::assert_eq;

#[test]
fn should_list_cycle_partners_for_each_directory_in_a_two_directory_cycle() {
    let report = report_with_graph(
        vec![
            node("api/handler.rs::handle", "api/handler.rs", "handle"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![
            Edge {
                from: "api/handler.rs::handle".to_string(),
                to: "store/db.rs::save".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "store/db.rs::save".to_string(),
                to: "api/handler.rs::handle".to_string(),
                is_cycle: false,
            },
        ],
    );

    let partners = cycle_partners(&report);

    let mut expected = HashMap::new();
    expected.insert("api".to_string(), vec!["store".to_string()]);
    expected.insert("store".to_string(), vec!["api".to_string()]);
    assert_eq!(expected, partners);
}

#[test]
fn should_return_empty_map_when_no_directory_is_in_a_cycle() {
    let report = report_with_graph(
        vec![
            node("api/a.rs::a", "api/a.rs", "a"),
            node("store/db.rs::save", "store/db.rs", "save"),
        ],
        vec![Edge {
            from: "api/a.rs::a".to_string(),
            to: "store/db.rs::save".to_string(),
            is_cycle: false,
        }],
    );

    let partners = cycle_partners(&report);

    let expected: HashMap<String, Vec<String>> = HashMap::new();
    assert_eq!(expected, partners);
}

#[test]
fn should_list_every_partner_when_three_directories_form_one_cycle() {
    // api -> store -> service -> api: a three-directory cycle, so every
    // directory's partner list must contain the other two.
    let report = report_with_graph(
        vec![
            node("api/a.rs::a", "api/a.rs", "a"),
            node("store/s.rs::s", "store/s.rs", "s"),
            node("service/v.rs::v", "service/v.rs", "v"),
        ],
        vec![
            Edge {
                from: "api/a.rs::a".to_string(),
                to: "store/s.rs::s".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "store/s.rs::s".to_string(),
                to: "service/v.rs::v".to_string(),
                is_cycle: false,
            },
            Edge {
                from: "service/v.rs::v".to_string(),
                to: "api/a.rs::a".to_string(),
                is_cycle: false,
            },
        ],
    );

    let partners = cycle_partners(&report);

    let mut expected = HashMap::new();
    expected.insert(
        "api".to_string(),
        vec!["service".to_string(), "store".to_string()],
    );
    expected.insert(
        "service".to_string(),
        vec!["api".to_string(), "store".to_string()],
    );
    expected.insert(
        "store".to_string(),
        vec!["api".to_string(), "service".to_string()],
    );
    assert_eq!(expected, partners);
}
