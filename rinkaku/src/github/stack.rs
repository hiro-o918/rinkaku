//! GitHub stacked-PR discovery (ADR 0075): the GraphQL `PullRequest.stack`
//! query, its parsed shape, and the `gh api graphql` shell-out.

/// One open layer of a stack, in the same field shape `PrInfo` carries
/// for a single PR plus the display fields the TUI's stack position needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StackPr {
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) head_ref_name: String,
    pub(crate) base_ref_name: String,
    pub(crate) base_ref_oid: String,
    pub(crate) head_ref_oid: String,
}

/// A stack's open layers, bottom (closest to trunk) first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrStack {
    pub(crate) number: u64,
    pub(crate) base_ref_name: String,
    pub(crate) prs: Vec<StackPr>,
}

impl PrStack {
    /// Index of `number` within `prs`, or `None` when the requested PR is
    /// not an open layer (a merged layer is dropped by `parse_stack_json`).
    pub(crate) fn position_of(&self, number: u64) -> Option<usize> {
        todo!("find the index of PR #{number} in the parsed stack")
    }
}

pub(crate) fn stack_query() -> &'static str {
    todo!("return the PullRequest.stack GraphQL query text")
}

/// Parses `gh api graphql`'s response for [`stack_query`]: `Ok(None)` when
/// the PR is not stacked, otherwise the open layers sorted by `position`.
pub(crate) fn parse_stack_json(json: &str) -> anyhow::Result<Option<PrStack>> {
    todo!("parse {json} into a PrStack, dropping merged/non-open entries")
}

pub(crate) fn fetch_pr_stack(
    owner: &str,
    repo: &str,
    number: u64,
) -> anyhow::Result<Option<PrStack>> {
    let output = std::process::Command::new("gh")
        .args(["api", "graphql", "-f"])
        .arg(format!("query={}", stack_query()))
        .args([
            "-F",
            &format!("owner={owner}"),
            "-F",
            &format!("name={repo}"),
        ])
        .args(["-F", &format!("number={number}")])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "gh api graphql (stack of {owner}/{repo}#{number}) failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    parse_stack_json(&String::from_utf8(output.stdout)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    #[ignore = "not implemented"]
    fn should_return_none_when_stack_is_null() {
        let json = r#"{"data":{"repository":{"pullRequest":{"number":249,"stack":null}}}}"#;

        let actual = parse_stack_json(json).unwrap();

        assert_eq!(None, actual);
    }

    #[test]
    #[ignore = "not implemented"]
    fn should_drop_merged_entries_and_sort_by_position_when_stack_has_mixed_states() {
        let json = r#"{"data":{"repository":{"pullRequest":{"number":43,"stack":{
            "number":7,"baseRefName":"main","size":3,"entries":{"nodes":[
              {"position":3,"pullRequest":{"number":44,"title":"frontend","headRefName":"frontend","baseRefName":"api","baseRefOid":"bbb","headRefOid":"ccc","state":"OPEN","merged":false,"isDraft":true}},
              {"position":1,"pullRequest":{"number":42,"title":"auth","headRefName":"auth","baseRefName":"main","baseRefOid":"000","headRefOid":"aaa","state":"MERGED","merged":true,"isDraft":false}},
              {"position":2,"pullRequest":{"number":43,"title":"api","headRefName":"api","baseRefName":"auth","baseRefOid":"aaa","headRefOid":"bbb","state":"OPEN","merged":false,"isDraft":false}}
            ]}}}}}}"#;

        let actual = parse_stack_json(json).unwrap();

        assert_eq!(
            Some(PrStack {
                number: 7,
                base_ref_name: "main".to_string(),
                prs: vec![
                    StackPr {
                        number: 43,
                        title: "api".to_string(),
                        head_ref_name: "api".to_string(),
                        base_ref_name: "auth".to_string(),
                        base_ref_oid: "aaa".to_string(),
                        head_ref_oid: "bbb".to_string(),
                    },
                    StackPr {
                        number: 44,
                        title: "frontend".to_string(),
                        head_ref_name: "frontend".to_string(),
                        base_ref_name: "api".to_string(),
                        base_ref_oid: "bbb".to_string(),
                        head_ref_oid: "ccc".to_string(),
                    },
                ],
            }),
            actual
        );
    }

    #[test]
    #[ignore = "not implemented"]
    fn should_error_when_json_is_malformed() {
        let actual = parse_stack_json("{not json");

        assert!(actual.is_err());
    }

    #[test]
    #[ignore = "not implemented"]
    fn should_error_when_response_carries_graphql_errors() {
        let json = r#"{"data":null,"errors":[{"message":"Could not resolve to a PullRequest"}]}"#;

        let actual = parse_stack_json(json);

        assert!(actual.is_err());
    }

    #[test]
    #[ignore = "not implemented"]
    fn should_find_cursor_position_when_requested_pr_is_an_open_layer() {
        let stack = PrStack {
            number: 7,
            base_ref_name: "main".to_string(),
            prs: vec![
                StackPr {
                    number: 43,
                    title: "api".to_string(),
                    head_ref_name: "api".to_string(),
                    base_ref_name: "auth".to_string(),
                    base_ref_oid: "aaa".to_string(),
                    head_ref_oid: "bbb".to_string(),
                },
                StackPr {
                    number: 44,
                    title: "frontend".to_string(),
                    head_ref_name: "frontend".to_string(),
                    base_ref_name: "api".to_string(),
                    base_ref_oid: "bbb".to_string(),
                    head_ref_oid: "ccc".to_string(),
                },
            ],
        };

        assert_eq!(Some(1), stack.position_of(44));
        assert_eq!(None, stack.position_of(42));
    }
}
