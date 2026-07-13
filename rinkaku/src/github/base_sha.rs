//! `--pr` mode base-SHA resolution (ADR 0007) and the `git fetch` helpers it drives.

/// Fetches PR `number`'s head ref into the repository at `cwd` and
/// returns the fetched commit's SHA, via
/// `git fetch origin refs/pull/<number>/head` followed by
/// `git rev-parse FETCH_HEAD`.
pub(crate) fn fetch_pr_head(number: u64, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
    run_git_fetch(&format!("refs/pull/{number}/head"), cwd)
}

/// Fetches branch `name` into the repository at `cwd` and returns the
/// fetched commit's SHA. Used to resolve `--pr` mode's base commit from
/// the base branch name `gh pr view` reports.
pub(crate) fn fetch_branch_head(
    name: &str,
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<String> {
    run_git_fetch(name, cwd)
}

/// Resolves `--pr` mode's diff base commit following ADR 0007's
/// availability cascade, preferring `base_ref_oid` (pinned to the PR-time
/// base, correct for both open and merged PRs) over the base branch's
/// current tip (correct only for open PRs, since a merged PR's base
/// branch has since advanced past it).
///
/// Cascade, each step only taken if the previous one didn't already
/// resolve `base_ref_oid` locally:
///
/// 1. `object_exists` (`git cat-file -e <oid>^{commit}`) — already have it.
/// 2. `fetch_base_branch` (`git fetch origin <base_ref_name>`, returning
///    the fetched tip's SHA) then re-check `object_exists` — an ordinary
///    branch fetch usually retrieves it, since `base_ref_oid` is normally
///    reachable from the base branch's history. A failure here (e.g. the
///    base branch was deleted after the PR merged, or renamed) is soft:
///    `log::warn!` and fall through to step 3 rather than aborting the
///    whole run — step 3 is exactly the recovery path for a base branch
///    that no longer leads to `base_ref_oid`, so a step-2 failure must not
///    short-circuit past it.
/// 3. `fetch_oid` (`git fetch origin <oid>`) then re-check `object_exists`
///    — covers a base branch that has since been force-pushed past it,
///    renamed, or deleted (including the case where step 2 itself failed
///    to fetch at all).
/// 4. Fall back to the base branch's tip with `used_fallback` signaling
///    the caller should warn — the commit is unreachable by any means
///    available, so this degrades rather than fails the whole run. Reuses
///    step 2's fetched tip when step 2 succeeded, rather than fetching the
///    same branch a second time; only calls `fetch_base_branch` again here
///    if step 2 itself failed (so there is no tip yet to reuse).
///
/// Every IO step is injected as a closure so this decision logic is
/// unit-testable without shelling out to `git`, following the same
/// pattern as `select_matching_clone` elsewhere in this file.
///
/// Returns the resolved SHA and whether the fallback (step 4) was used.
pub(crate) fn resolve_pr_base_sha(
    base_ref_oid: &str,
    mut object_exists: impl FnMut(&str) -> bool,
    mut fetch_base_branch: impl FnMut() -> anyhow::Result<String>,
    mut fetch_oid: impl FnMut(&str) -> anyhow::Result<()>,
) -> anyhow::Result<(String, bool)> {
    if object_exists(base_ref_oid) {
        return Ok((base_ref_oid.to_string(), false));
    }

    let branch_tip = match fetch_base_branch() {
        Ok(tip) => {
            if object_exists(base_ref_oid) {
                return Ok((base_ref_oid.to_string(), false));
            }
            Some(tip)
        }
        Err(source) => {
            log::warn!(
                "fetching the base branch failed, continuing the base-commit resolution \
                 cascade: {source}"
            );
            None
        }
    };

    if fetch_oid(base_ref_oid).is_ok() && object_exists(base_ref_oid) {
        return Ok((base_ref_oid.to_string(), false));
    }

    let branch_tip = match branch_tip {
        Some(tip) => tip,
        None => fetch_base_branch()?,
    };
    Ok((branch_tip, true))
}

/// Runs `git cat-file -e <oid>^{commit}` in `cwd`, i.e. whether `oid`
/// already exists locally as a commit object — the cheap first check in
/// `resolve_pr_base_sha`'s cascade, run before attempting any fetch.
pub(crate) fn object_exists_locally(cwd: Option<&std::path::Path>, oid: &str) -> bool {
    let mut command = std::process::Command::new("git");
    command.args(["cat-file", "-e", &format!("{oid}^{{commit}}")]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command.output().is_ok_and(|output| output.status.success())
}

/// Runs `git fetch origin <oid>` in `cwd` — the direct-oid step of
/// `resolve_pr_base_sha`'s cascade, tried only when the base branch itself
/// (already fetched by the caller) didn't bring the commit in, e.g. after
/// a force-push past it. Unlike `run_git_fetch`, this doesn't need
/// `FETCH_HEAD` afterwards: the caller re-checks `object_exists_locally`
/// instead, since fetching a bare oid doesn't update any ref.
pub(crate) fn fetch_oid(cwd: Option<&std::path::Path>, oid: &str) -> anyhow::Result<()> {
    let mut command = std::process::Command::new("git");
    command.args(["fetch", "origin", oid]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git fetch origin {oid} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Runs `git fetch origin <refspec>` then `git rev-parse FETCH_HEAD` in
/// the repository at `cwd`, returning the resulting SHA. Shared by
/// `fetch_pr_head` and `fetch_branch_head`, which differ only in what
/// refspec they fetch.
///
/// `cwd` selects the repository to run `git` in; `None` uses the
/// process's current directory (production cwd-clone callers),
/// `Some(dir)` pins it (cache clones, tests) — same rationale as
/// `read_git_show_file`'s `cwd`.
fn run_git_fetch(refspec: &str, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
    let mut fetch_command = std::process::Command::new("git");
    fetch_command.args(["fetch", "origin", refspec]);
    if let Some(cwd) = cwd {
        fetch_command.current_dir(cwd);
    }
    let fetch_output = fetch_command.output()?;
    if !fetch_output.status.success() {
        anyhow::bail!(
            "git fetch origin {refspec} failed: {}",
            String::from_utf8_lossy(&fetch_output.stderr)
        );
    }

    let mut rev_parse_command = std::process::Command::new("git");
    rev_parse_command.args(["rev-parse", "FETCH_HEAD"]);
    if let Some(cwd) = cwd {
        rev_parse_command.current_dir(cwd);
    }
    let rev_parse_output = rev_parse_command.output()?;
    if !rev_parse_output.status.success() {
        anyhow::bail!(
            "git rev-parse FETCH_HEAD failed after fetching {refspec}: {}",
            String::from_utf8_lossy(&rev_parse_output.stderr)
        );
    }
    Ok(String::from_utf8(rev_parse_output.stdout)?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod resolve_pr_base_sha_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use std::cell::RefCell;

        #[test]
        fn should_return_base_ref_oid_when_it_already_exists_locally() {
            let fetch_base_branch_calls = RefCell::new(0);
            let fetch_oid_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| true,
                || {
                    *fetch_base_branch_calls.borrow_mut() += 1;
                    Ok("branch-tip-sha".to_string())
                },
                |_oid| {
                    *fetch_oid_calls.borrow_mut() += 1;
                    Ok(())
                },
            )
            .expect("should resolve without error");

            assert_eq!(("base789".to_string(), false), actual);
            assert_eq!(0, *fetch_base_branch_calls.borrow());
            assert_eq!(0, *fetch_oid_calls.borrow());
        }

        #[test]
        fn should_return_base_ref_oid_when_fetching_the_base_branch_makes_it_available() {
            let exists_calls = RefCell::new(0);
            let object_exists = |_oid: &str| {
                let mut calls = exists_calls.borrow_mut();
                *calls += 1;
                // First check (before any fetch) fails; the check right
                // after `fetch_base_branch` succeeds.
                *calls > 1
            };

            let actual = resolve_pr_base_sha(
                "base789",
                object_exists,
                || Ok("branch-tip-sha".to_string()),
                |_oid| panic!("fetch_oid must not be called when the base branch fetch sufficed"),
            )
            .expect("should resolve without error");

            assert_eq!(("base789".to_string(), false), actual);
        }

        #[test]
        fn should_return_base_ref_oid_when_fetching_the_oid_directly_makes_it_available() {
            let exists_calls = RefCell::new(0);
            let object_exists = |_oid: &str| {
                let mut calls = exists_calls.borrow_mut();
                *calls += 1;
                // Neither the initial check nor the one after the base
                // branch fetch succeed; only the one after `fetch_oid`
                // does (third call).
                *calls > 2
            };

            let actual = resolve_pr_base_sha(
                "base789",
                object_exists,
                || Ok("branch-tip-sha".to_string()),
                |_oid| Ok(()),
            )
            .expect("should resolve without error");

            assert_eq!(("base789".to_string(), false), actual);
        }

        #[test]
        fn should_fall_back_to_branch_tip_when_the_oid_is_unreachable_by_any_means() {
            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || Ok("branch-tip-sha".to_string()),
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
        }

        #[test]
        fn should_fall_back_to_branch_tip_when_fetch_oid_succeeds_but_object_still_missing() {
            // `git fetch origin <oid>` can itself succeed (e.g. the remote
            // accepts the request) while the object is still not resolvable
            // locally afterwards — covered separately from the "fetch_oid
            // errors outright" case above.
            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || Ok("branch-tip-sha".to_string()),
                |_oid| Ok(()),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
        }

        // Regression test for the must-fix correctness bug: a step-2
        // fetch failure (e.g. the base branch was deleted or renamed after
        // the PR merged) must not abort the whole cascade — step 3 (fetch
        // the oid directly) is exactly the recovery path for this
        // situation, so it must still run and can still resolve
        // `base_ref_oid` even though step 2 failed.
        #[test]
        fn should_fall_through_to_fetch_oid_when_fetching_the_base_branch_fails() {
            let exists_calls = RefCell::new(0);
            let object_exists = |_oid: &str| {
                let mut calls = exists_calls.borrow_mut();
                *calls += 1;
                // Only the initial check happens before the failed branch
                // fetch (which does not re-check); the check after
                // `fetch_oid` (second call) succeeds.
                *calls > 1
            };

            let actual = resolve_pr_base_sha(
                "base789",
                object_exists,
                || anyhow::bail!("simulated: base branch was deleted"),
                |_oid| Ok(()),
            )
            .expect("a step-2 failure must not abort the cascade");

            assert_eq!(("base789".to_string(), false), actual);
        }

        // Sibling case: if step 3 also can't resolve the oid after a
        // step-2 failure, the cascade must still fall back (step 4) rather
        // than propagating the step-2 error — step 2's failure was already
        // handled by falling through, not by failing the whole call.
        #[test]
        fn should_fetch_branch_tip_for_fallback_when_step_two_failed_and_fetch_oid_also_fails() {
            let fetch_base_branch_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || {
                    let mut calls = fetch_base_branch_calls.borrow_mut();
                    *calls += 1;
                    if *calls == 1 {
                        anyhow::bail!("simulated: base branch was deleted")
                    } else {
                        // Step 4 must re-fetch since step 2 never produced
                        // a tip to reuse.
                        Ok("branch-tip-sha".to_string())
                    }
                },
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
            assert_eq!(2, *fetch_base_branch_calls.borrow());
        }

        // Regression test for the must-fix cleanup: when step 2 succeeded
        // (returned a tip) but didn't make `base_ref_oid` resolvable, and
        // step 3 also fails, step 4's fallback must reuse step 2's tip
        // rather than fetching the same base branch a second time.
        #[test]
        fn should_reuse_step_two_tip_for_fallback_without_refetching() {
            let fetch_base_branch_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || {
                    *fetch_base_branch_calls.borrow_mut() += 1;
                    Ok("branch-tip-sha".to_string())
                },
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
            )
            .expect("should fall back rather than error");

            assert_eq!(("branch-tip-sha".to_string(), true), actual);
            assert_eq!(
                1,
                *fetch_base_branch_calls.borrow(),
                "fetch_base_branch must only be called once (by step 2); step 4 must reuse its \
                 result instead of fetching the base branch again"
            );
        }

        #[test]
        fn should_propagate_error_when_the_branch_tip_fallback_itself_fails() {
            let fetch_base_branch_calls = RefCell::new(0);

            let actual = resolve_pr_base_sha(
                "base789",
                |_oid| false,
                || {
                    let mut calls = fetch_base_branch_calls.borrow_mut();
                    *calls += 1;
                    anyhow::bail!("simulated: git fetch origin main failed")
                },
                |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
            );

            assert!(actual.is_err());
        }
    }
}
