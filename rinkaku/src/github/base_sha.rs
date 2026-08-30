//! `--pr` mode base-SHA resolution (ADR 0007) and the `git fetch` helpers it drives.

/// Fetches PR `number`'s head into the repository at `cwd` and returns
/// the fetched commit's SHA, via `fetch_pr_heads`'s named-ref fetch — see
/// its doc comment for why `FETCH_HEAD` is unsafe here too.
pub(crate) fn fetch_pr_head(number: u64, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
    fetch_pr_heads(&[number], cwd)?
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("fetch_pr_heads returned no SHA for PR #{number}"))
}

/// Fetches every PR head in `numbers` with one multi-refspec `git fetch`
/// into `refs/rinkaku/pull/<number>/head` and returns their SHAs in the
/// same order. Named refs rather than `FETCH_HEAD`: another rinkaku
/// process fetching in the same clone overwrites `FETCH_HEAD` between a
/// fetch and its `rev-parse`, and a stack session fetches several heads
/// in a row, so that window is wide enough to hit.
pub(crate) fn fetch_pr_heads(
    numbers: &[u64],
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<Vec<String>> {
    if numbers.is_empty() {
        return Ok(Vec::new());
    }
    let refspecs: Vec<String> = numbers
        .iter()
        .map(|number| format!("+refs/pull/{number}/head:{}", local_pr_head_ref(*number)))
        .collect();
    let mut fetch_command = std::process::Command::new("git");
    fetch_command.args(["fetch", "origin"]).args(&refspecs);
    if let Some(cwd) = cwd {
        fetch_command.current_dir(cwd);
    }
    let fetch_output = fetch_command.output()?;
    if !fetch_output.status.success() {
        anyhow::bail!(
            "git fetch origin {} failed: {}",
            refspecs.join(" "),
            String::from_utf8_lossy(&fetch_output.stderr)
        );
    }
    numbers
        .iter()
        .map(|number| rev_parse(&local_pr_head_ref(*number), cwd))
        .collect()
}

fn local_pr_head_ref(number: u64) -> String {
    format!("refs/rinkaku/pull/{number}/head")
}

fn rev_parse(reference: &str, cwd: Option<&std::path::Path>) -> anyhow::Result<String> {
    let mut command = std::process::Command::new("git");
    command.args(["rev-parse", reference]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git rev-parse {reference} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
    mod fetch_pr_heads_tests {
        use crate::github::base_sha::fetch_pr_heads;
        use crate::test_util::{init_repo_with_committed_file, run_git};
        use pretty_assertions::assert_eq;

        fn head_sha(dir: &std::path::Path) -> String {
            let output = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .expect("git rev-parse");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        #[test]
        fn should_return_each_pr_head_in_input_order_when_fetched_in_one_call() {
            let remote = tempfile::TempDir::new().expect("remote dir");
            init_repo_with_committed_file(remote.path(), "fn one() {}\n");
            let first = head_sha(remote.path());
            run_git(remote.path(), &["update-ref", "refs/pull/7/head", &first]);
            std::fs::write(remote.path().join("src/lib.rs"), "fn two() {}\n").expect("write");
            run_git(remote.path(), &["commit", "-am", "second"]);
            let second = head_sha(remote.path());
            run_git(remote.path(), &["update-ref", "refs/pull/9/head", &second]);
            let clone = tempfile::TempDir::new().expect("clone dir");
            let clone_dir = clone.path().join("repo");
            run_git(
                clone.path(),
                &[
                    "clone",
                    "--quiet",
                    remote.path().to_str().expect("utf8 path"),
                    clone_dir.to_str().expect("utf8 path"),
                ],
            );

            let actual = fetch_pr_heads(&[9, 7], Some(&clone_dir)).expect("fetch");

            assert_eq!(vec![second, first], actual);
        }

        #[test]
        fn should_return_empty_without_fetching_when_no_numbers_are_given() {
            let dir = tempfile::TempDir::new().expect("dir");

            let actual = fetch_pr_heads(&[], Some(dir.path())).expect("fetch");

            assert_eq!(Vec::<String>::new(), actual);
        }
    }

    mod fetch_pr_head_tests {
        use crate::github::base_sha::fetch_pr_head;
        use crate::test_util::{init_repo_with_committed_file, run_git};
        use pretty_assertions::assert_eq;

        fn head_sha(dir: &std::path::Path) -> String {
            let output = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(dir)
                .output()
                .expect("git rev-parse");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        #[test]
        fn should_return_the_pr_head_when_fetched_into_a_named_ref() {
            let remote = tempfile::TempDir::new().expect("remote dir");
            init_repo_with_committed_file(remote.path(), "fn one() {}\n");
            let expected = head_sha(remote.path());
            run_git(
                remote.path(),
                &["update-ref", "refs/pull/7/head", &expected],
            );
            let clone = tempfile::TempDir::new().expect("clone dir");
            let clone_dir = clone.path().join("repo");
            run_git(
                clone.path(),
                &[
                    "clone",
                    "--quiet",
                    remote.path().to_str().expect("utf8 path"),
                    clone_dir.to_str().expect("utf8 path"),
                ],
            );

            let actual = fetch_pr_head(7, Some(&clone_dir)).expect("fetch");

            assert_eq!(expected, actual);
            let ref_output = std::process::Command::new("git")
                .args(["rev-parse", "refs/rinkaku/pull/7/head"])
                .current_dir(&clone_dir)
                .output()
                .expect("git rev-parse");
            assert_eq!(expected, String::from_utf8_lossy(&ref_output.stdout).trim());
        }
    }

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
