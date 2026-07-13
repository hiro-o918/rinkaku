//! PR workdir resolution: cwd / ghq / cache clone probe cascade (ADR 0005, 0006),
//! plus the helpers each step needs.

use crate::github::pr_arg::PrArg;
use crate::github::remote::{git_remote_origin_url, github_remote_matches};

/// Determines which repository `--pr` mode should run its `git` commands
/// in, and clones one into the cache if needed. Probes in order (ADR
/// 0006): (1) the current directory, (2) a ghq-managed clone, (3) the
/// cache clone (ADR 0005).
///
/// `PrArg::Number` always uses the process's current directory (`None`,
/// meaning "no override" to the `cwd` parameters downstream) — a bare
/// number carries no repository information, so ADR 0004's "run inside a
/// local clone" requirement is unchanged for it.
///
/// `PrArg::Url` first checks whether the current directory is already a
/// clone of that repository (`git remote get-url origin` matching
/// `owner`/`repo`, case-insensitively via `github_remote_matches`); if so
/// it also returns `None`, reusing the cwd exactly like `PrArg::Number`
/// does today. Otherwise it asks `ghq list --full-path --exact
/// <owner>/<repo>` for candidate clones (`ghq_candidate_clones`) and
/// picks the first whose real origin matches (`select_matching_clone`,
/// resolving each candidate's origin via `git_remote_origin_url`); ghq
/// being absent, erroring, or returning only non-matching clones all
/// fall through silently to this step returning `None` (per ADR 0006 —
/// `ghq_candidate_clones` already logs the reason at debug level). Only
/// if neither the cwd nor a ghq clone matches does it fall back to the
/// per-repository cache directory (`cache_repo_dir`, reading the real
/// environment here at the boundary) and clone into it if it doesn't
/// exist yet — an existing cache entry, like a discovered ghq clone, is
/// left alone here and refreshed by the `git fetch` calls `main` makes
/// afterwards, not re-cloned.
pub(crate) fn resolve_pr_workdir(parsed: &PrArg) -> anyhow::Result<Option<std::path::PathBuf>> {
    let PrArg::Url { owner, repo, .. } = parsed else {
        return Ok(None);
    };

    if let Some(origin) = git_remote_origin_url(None)?
        && github_remote_matches(&origin, owner, repo)
    {
        log::info!("using the current directory as a clone of {owner}/{repo}");
        return Ok(None);
    }

    let ghq_candidates = ghq_candidate_clones(owner, repo);
    if let Some(discovered) = select_matching_clone(
        &ghq_candidates,
        |path| git_remote_origin_url(Some(path)).ok().flatten(),
        owner,
        repo,
    ) {
        log::info!(
            "using ghq-managed clone of {owner}/{repo} at {}",
            discovered.display()
        );
        return Ok(Some(discovered));
    }

    let dir = cache_repo_dir(
        std::env::var("RINKAKU_CACHE_DIR").ok().as_deref(),
        std::env::var("XDG_CACHE_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
        owner,
        repo,
    )?;
    if !dir.exists() {
        // Create the parent (`.../repos/github.com/<owner>/`) only, not
        // `dir` itself: `gh repo clone` (like `git clone`) creates its
        // destination directory and fails if it already exists.
        std::fs::create_dir_all(dir.parent().unwrap_or(&dir)).map_err(|source| {
            anyhow::anyhow!(
                "failed to create cache directory for {}: {source}",
                dir.display()
            )
        })?;
        log::info!(
            "cloning {owner}/{repo} into cache at {} (first run against this repository)",
            dir.display()
        );
        clone_repo_into_cache(owner, repo, &dir)?;
    } else {
        log::info!("using cache clone of {owner}/{repo} at {}", dir.display());
    }
    Ok(Some(dir))
}

/// Resolves the root cache directory for `--pr` URL auto-clones (ADR
/// 0005), then the per-repository clone path under it.
///
/// Precedence for the root, evaluated in order: `rinkaku_cache_dir`
/// (`$RINKAKU_CACHE_DIR`) if set, else `<xdg_cache_home>/rinkaku`
/// (`$XDG_CACHE_HOME/rinkaku`) if set, else `<home>/.cache/rinkaku`. An
/// error if none of the three inputs is available — there is then no
/// sane place to put the cache. All three are taken as arguments rather
/// than read from the environment here, so this stays a pure function the
/// precedence order can be unit-tested against directly; `main` is the
/// only place that reads the actual environment.
///
/// Repo layout under the root: `repos/github.com/<owner>/<repo>`, so
/// different git hosts (a future extension) could share one cache root
/// without path collisions, and so the cache directory's own contents
/// read as self-explanatory if a user goes looking (`~/.cache/rinkaku/
/// repos/github.com/octocat/hello-world`).
pub(crate) fn cache_repo_dir(
    rinkaku_cache_dir: Option<&str>,
    xdg_cache_home: Option<&str>,
    home: Option<&str>,
    owner: &str,
    repo: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let root = if let Some(dir) = rinkaku_cache_dir {
        std::path::PathBuf::from(dir)
    } else if let Some(dir) = xdg_cache_home {
        std::path::PathBuf::from(dir).join("rinkaku")
    } else if let Some(dir) = home {
        std::path::PathBuf::from(dir).join(".cache").join("rinkaku")
    } else {
        anyhow::bail!(
            "cannot determine a cache directory for --pr: set $RINKAKU_CACHE_DIR, \
             $XDG_CACHE_HOME, or $HOME"
        );
    };
    Ok(root.join("repos").join("github.com").join(owner).join(repo))
}

/// Clones `owner/repo` as a blobless partial clone (`--filter=blob:none`,
/// ADR 0005) into `dir` via `gh repo clone`, delegating authentication to
/// `gh` (ADR 0004's stance, applied to cloning too). Only called when
/// `dir` does not already exist — an existing cache entry is refreshed by
/// the ordinary `git fetch` calls in `main` instead of being re-cloned.
pub(crate) fn clone_repo_into_cache(
    owner: &str,
    repo: &str,
    dir: &std::path::Path,
) -> anyhow::Result<()> {
    let slug = format!("{owner}/{repo}");
    let output = std::process::Command::new("gh")
        .args([
            "repo",
            "clone",
            &slug,
            &dir.to_string_lossy(),
            "--",
            "--filter=blob:none",
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "gh repo clone {slug} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Parses `ghq list --full-path --exact <owner>/<repo>`'s stdout into
/// candidate clone paths: one per non-blank line, trimmed. `ghq` prints
/// one absolute path per line and nothing else on success, but blank
/// lines (a trailing newline, or possibly a stray empty line) are
/// filtered out defensively rather than turned into a bogus empty-path
/// candidate.
pub(crate) fn parse_ghq_list_output(stdout: &str) -> Vec<std::path::PathBuf> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(std::path::PathBuf::from)
        .collect()
}

/// Runs `ghq list --full-path --exact <owner>/<repo>` and returns the
/// candidate clone paths it reports (ADR 0006's ghq-discovery probe,
/// step 2 between the cwd check and the cache fallback).
///
/// Always returns an empty `Vec` rather than an `Err` — never a fatal
/// error for `--pr` mode — for every way this can fail to find a usable
/// clone: `ghq` missing from `PATH` (`Command::output` fails with
/// `io::ErrorKind::NotFound`), a non-zero exit (e.g. `ghq` installed but
/// its own config is broken), or a zero exit with no matching clones.
/// ADR 0006 is explicit that all of these "fall through silently to the
/// cache"; a `log::debug!` records which case fired, for anyone who wants
/// to know why a clone wasn't discovered without it being an error.
pub(crate) fn ghq_candidate_clones(owner: &str, repo: &str) -> Vec<std::path::PathBuf> {
    let slug = format!("{owner}/{repo}");
    let output = match std::process::Command::new("ghq")
        .args(["list", "--full-path", "--exact", &slug])
        .output()
    {
        Ok(output) => output,
        Err(source) => {
            log::debug!("ghq not runnable, falling back to cache for {slug}: {source}");
            return Vec::new();
        }
    };
    if !output.status.success() {
        log::debug!(
            "ghq list {slug} exited non-zero, falling back to cache: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Vec::new();
    }
    match String::from_utf8(output.stdout) {
        Ok(stdout) => parse_ghq_list_output(&stdout),
        Err(source) => {
            log::debug!(
                "ghq list {slug} produced non-UTF-8 output, falling back to cache: {source}"
            );
            Vec::new()
        }
    }
}

/// Picks the first of `candidates` whose origin (resolved by the injected
/// `origin_of` port) matches `owner`/`repo` per `github_remote_matches`.
/// `None` if `candidates` is empty or none of them match.
///
/// `origin_of` is injected — production wires it to
/// `|path| git_remote_origin_url(Some(path)).ok().flatten()`, tests wire
/// it to an in-memory map — so this selection logic is unit-testable
/// without shelling out to `git`, following the same read-file-port style
/// as `analyze_diff`/`build_resolver` elsewhere in this file.
pub(crate) fn select_matching_clone(
    candidates: &[std::path::PathBuf],
    origin_of: impl Fn(&std::path::Path) -> Option<String>,
    owner: &str,
    repo: &str,
) -> Option<std::path::PathBuf> {
    candidates
        .iter()
        .find(|candidate| {
            origin_of(candidate).is_some_and(|origin| github_remote_matches(&origin, owner, repo))
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::commands::resolve_repo_root;
    use crate::test_util::init_repo_with_committed_file;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_prefer_rinkaku_cache_dir_when_set(
        Some("/custom/cache"),
        Some("/xdg/cache"),
        Some("/home/user"),
        "/custom/cache/repos/github.com/octocat/hello-world"
    )]
    #[case::should_fall_back_to_xdg_cache_home_when_rinkaku_cache_dir_unset(
        None,
        Some("/xdg/cache"),
        Some("/home/user"),
        "/xdg/cache/rinkaku/repos/github.com/octocat/hello-world"
    )]
    #[case::should_fall_back_to_home_when_neither_env_var_set(
        None,
        None,
        Some("/home/user"),
        "/home/user/.cache/rinkaku/repos/github.com/octocat/hello-world"
    )]
    fn should_build_cache_repo_dir(
        #[case] rinkaku_cache_dir: Option<&str>,
        #[case] xdg_cache_home: Option<&str>,
        #[case] home: Option<&str>,
        #[case] expected: &str,
    ) {
        let actual = cache_repo_dir(
            rinkaku_cache_dir,
            xdg_cache_home,
            home,
            "octocat",
            "hello-world",
        )
        .expect("expected a cache directory to be resolved");

        assert_eq!(std::path::PathBuf::from(expected), actual);
    }

    #[test]
    fn should_fail_to_build_cache_repo_dir_when_no_env_source_is_available() {
        let actual = cache_repo_dir(None, None, None, "octocat", "hello-world");

        assert!(actual.is_err());
    }

    #[rstest]
    #[case::should_parse_single_line(
        "/home/user/ghq/github.com/octocat/hello-world\n",
        vec![std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world")]
    )]
    #[case::should_parse_multiple_lines(
        "/home/user/ghq/github.com/octocat/hello-world\n/home/user/work/hello-world\n",
        vec![
            std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world"),
            std::path::PathBuf::from("/home/user/work/hello-world"),
        ]
    )]
    #[case::should_skip_blank_lines_between_entries(
        "/home/user/ghq/github.com/octocat/hello-world\n\n/home/user/work/hello-world\n",
        vec![
            std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world"),
            std::path::PathBuf::from("/home/user/work/hello-world"),
        ]
    )]
    #[case::should_trim_surrounding_whitespace_per_line(
        "  /home/user/ghq/github.com/octocat/hello-world  \n",
        vec![std::path::PathBuf::from("/home/user/ghq/github.com/octocat/hello-world")]
    )]
    #[case::should_return_empty_vec_for_empty_string("", vec![])]
    #[case::should_return_empty_vec_for_whitespace_only_string("\n\n  \n", vec![])]
    fn should_parse_ghq_list_output(
        #[case] stdout: &str,
        #[case] expected: Vec<std::path::PathBuf>,
    ) {
        let actual = parse_ghq_list_output(stdout);

        assert_eq!(expected, actual);
    }

    #[rstest]
    #[case::should_return_first_candidate_when_it_matches(
        vec!["/a", "/b"],
        vec![("/a", "https://github.com/octocat/hello-world.git")],
        Some(std::path::PathBuf::from("/a"))
    )]
    #[case::should_return_later_candidate_when_earlier_ones_mismatch(
        vec!["/a", "/b", "/c"],
        vec![
            ("/a", "https://github.com/someone-else/other-repo.git"),
            ("/b", "https://github.com/octocat/hello-world.git"),
        ],
        Some(std::path::PathBuf::from("/b"))
    )]
    #[case::should_return_none_when_no_candidate_matches(
        vec!["/a", "/b"],
        vec![
            ("/a", "https://github.com/someone-else/other-repo.git"),
            ("/b", "https://gitlab.com/octocat/hello-world.git"),
        ],
        None
    )]
    #[case::should_return_none_when_candidates_is_empty(vec![], vec![], None)]
    #[case::should_return_none_when_origin_lookup_yields_nothing_for_any_candidate(
        vec!["/a"],
        vec![],
        None
    )]
    fn should_select_matching_clone(
        #[case] candidates: Vec<&str>,
        #[case] origins: Vec<(&str, &str)>,
        #[case] expected: Option<std::path::PathBuf>,
    ) {
        let candidates: Vec<std::path::PathBuf> = candidates
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        let origin_of = |path: &std::path::Path| {
            origins
                .iter()
                .find(|(candidate_path, _)| std::path::Path::new(candidate_path) == path)
                .map(|(_, origin)| origin.to_string())
        };

        let actual = select_matching_clone(&candidates, origin_of, "octocat", "hello-world");

        assert_eq!(expected, actual);
    }

    // Regression test for the review blocker: `main`'s `DisplayMode::Tui`
    // arm used to call `resolve_repo_root(None)` unconditionally, ignoring
    // `--pr`'s resolved `workdir` (a ghq/cache clone that can be anywhere
    // on disk, `resolve_pr_workdir`). If the process's own current
    // directory happened to be a *different* git repository, the TUI's
    // source view would silently resolve *that* repository's root and read
    // whatever unrelated file sits at the same relative path there,
    // instead of erroring or reading the PR's actual clone.
    //
    // This proves the mechanism `main` must use (pass the resolved
    // `workdir`/`cwd`, not `None`, whenever the `Report` was built from
    // one) actually reaches the other repository rather than falling back
    // to `pr_repo`: two independent repositories are set up, each with its
    // own `src/lib.rs` at the same relative path, and `resolve_repo_root`
    // is called with `pr_repo`'s path while the *process's* current
    // directory is left at `process_repo` — `resolve_repo_root(None)`
    // (the pre-fix call) would resolve `process_repo`, while
    // `resolve_repo_root(Some(pr_repo))` (the fix) must resolve `pr_repo`.
    #[test]
    fn should_resolve_pr_workdir_root_not_process_cwd_repo_when_both_are_git_repos() {
        let process_repo = tempfile::TempDir::new().expect("create process repo tempdir");
        init_repo_with_committed_file(process_repo.path(), "fn process_repo_marker() {}\n");
        let pr_repo = tempfile::TempDir::new().expect("create pr repo tempdir");
        init_repo_with_committed_file(pr_repo.path(), "fn pr_repo_marker() {}\n");

        // `None` (the pre-fix call the blocker flagged) would have resolved
        // wherever the test process's actual cwd happens to sit — not
        // asserted here since that is the whole point of the bug, only
        // exercised for contrast against the fixed call below.
        let actual = resolve_repo_root(Some(pr_repo.path()));

        let expected = pr_repo
            .path()
            .canonicalize()
            .expect("canonicalize expected");
        let actual = actual.canonicalize().expect("canonicalize actual");
        assert_eq!(expected, actual);
        assert_ne!(
            process_repo
                .path()
                .canonicalize()
                .expect("canonicalize process_repo"),
            actual,
            "must not resolve the unrelated process-cwd repository"
        );
    }
}
