//! Validated `--pr` argument parsing: bare PR numbers and GitHub PR URLs.

/// A validated `--pr` argument. `Url` carries `owner`/`repo` (not just the
/// PR number) so callers can decide, per ADR 0005, whether the current
/// directory's clone matches the PR's repository or a cache clone is
/// needed — information a bare `Number` inherently cannot provide, which
/// is exactly why `Number` still requires running inside a local clone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrArg {
    Number(u64),
    Url {
        owner: String,
        repo: String,
        number: u64,
    },
}

impl PrArg {
    /// The PR number, regardless of which variant this is. Used to build
    /// the `refs/pull/<number>/head` fetch refspec, which only needs the
    /// number even for `Url`.
    pub(crate) fn number(&self) -> u64 {
        match self {
            PrArg::Number(number) => *number,
            PrArg::Url { number, .. } => *number,
        }
    }
}

/// Extracts a validated `--pr` argument: either a bare number (`"76"`) or
/// a GitHub PR URL
/// (`https://github.com/<owner>/<repo>/pull/<number>`, tolerating a
/// trailing slash or extra path segments like `/files`).
///
/// `0` is rejected even though it parses as a `u64`: GitHub PR numbers
/// are 1-indexed, so `0` can only be a typo, and failing fast here beats
/// a confusing `gh pr view 0` error downstream.
pub(crate) fn parse_pr_arg(value: &str) -> anyhow::Result<PrArg> {
    match value.trim().strip_prefix("https://github.com/") {
        Some(rest) => {
            // Expect `<owner>/<repo>/pull/<number>[/...]`.
            let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
            match segments.as_slice() {
                [owner, repo, "pull", number, ..] => Ok(PrArg::Url {
                    owner: owner.to_string(),
                    repo: repo.to_string(),
                    number: parse_positive_pr_number(number, value)?,
                }),
                _ => anyhow::bail!(
                    "--pr URL must look like https://github.com/<owner>/<repo>/pull/<number>, \
                     got: {value}"
                ),
            }
        }
        None => Ok(PrArg::Number(parse_positive_pr_number(
            value.trim(),
            value,
        )?)),
    }
}

/// Parses `candidate` as a positive `u64` PR number, reporting errors
/// against the original (untrimmed/un-extracted) `--pr` value so the user
/// sees what they actually typed.
fn parse_positive_pr_number(candidate: &str, original_value: &str) -> anyhow::Result<u64> {
    let number: u64 = candidate.parse().map_err(|_| {
        anyhow::anyhow!("--pr must be a PR number or a GitHub PR URL, got: {original_value}")
    })?;
    if number == 0 {
        anyhow::bail!("--pr must be a positive PR number, got: {original_value}");
    }
    Ok(number)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_parse_bare_number("76", PrArg::Number(76))]
    #[case::should_parse_number_with_surrounding_whitespace(" 76 ", PrArg::Number(76))]
    #[case::should_parse_pull_url(
        "https://github.com/octocat/hello-world/pull/123",
        PrArg::Url {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            number: 123,
        }
    )]
    #[case::should_parse_pull_url_with_trailing_slash(
        "https://github.com/octocat/hello-world/pull/123/",
        PrArg::Url {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            number: 123,
        }
    )]
    #[case::should_parse_pull_url_with_extra_path_segment(
        "https://github.com/octocat/hello-world/pull/123/files",
        PrArg::Url {
            owner: "octocat".to_string(),
            repo: "hello-world".to_string(),
            number: 123,
        }
    )]
    fn should_parse_pr_arg_when_input_is_valid(#[case] input: &str, #[case] expected: PrArg) {
        let actual = parse_pr_arg(input).expect("expected a valid PR arg");

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_reject_empty_string("")]
    #[case::should_reject_non_numeric_string("abc")]
    #[case::should_reject_zero("0")]
    #[case::should_reject_negative_number("-1")]
    #[case::should_reject_non_pull_github_url("https://github.com/octocat/hello-world/issues/123")]
    #[case::should_reject_github_url_missing_number("https://github.com/octocat/hello-world/pull/")]
    #[case::should_reject_unrelated_url("https://example.com/pull/123")]
    fn should_reject_pr_arg_when_input_is_invalid(#[case] input: &str) {
        let actual = parse_pr_arg(input);

        assert!(actual.is_err(), "expected an error for input: {input}");
    }
}
