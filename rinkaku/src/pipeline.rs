//! `--base`/`--pr` pipeline extracted from `main.rs`.
//!
//! Hosts the end-to-end functions that turn a `(base, head, cwd)` triple
//! into a rendered `Report`: `run_base_pipeline` drives the diff → analyze
//! → resolve chain, with `changed_paths`, `resolve_generated_paths`,
//! `build_resolver`, and `read_stdin_diff` as supporting steps.

use crate::cli::Cli;
use crate::generated_paths::{check_generated_paths, check_generated_paths_batch};
use crate::git::cat_file_batch::read_git_show_files_batch;
use crate::git::commands::{list_git_files, run_git_diff};
use crate::git::file_read::{read_git_show_file, read_working_tree_file};
use crate::notes::garbage_input_note;
use crate::progress::AnalysisProgress;
use crate::spinner::AnalysisPhase;
use rinkaku_core::deps::TagsResolver;
use rinkaku_core::language::language_for_path;
use rinkaku_core::pipeline::analyze_diff;
use std::io::{IsTerminal, Read};

pub(crate) fn run_base_pipeline(
    cli: &Cli,
    base: &str,
    head: &str,
    cwd: Option<&std::path::Path>,
    progress: &dyn AnalysisProgress,
) -> anyhow::Result<(rinkaku_core::render::Report, String)> {
    log::debug!("diffing {base}...{head}");
    progress.set_phase(AnalysisPhase::Diffing);
    let diff_text = run_git_diff(base, head, cwd)?;
    if diff_text.trim().is_empty() {
        // ADR 0033: routed through `progress.note` rather than a bare
        // `eprintln!` — see `AnalysisProgress::note`'s own doc comment for
        // why (a raw stderr write here would interleave into the TUI's
        // alternate-screen frame stream mid-redraw during `--tui` mode).
        progress.note("note: diff is empty, nothing to analyze".to_string());
        return Ok((
            rinkaku_core::render::Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: Vec::new(),
                skipped: Vec::new(),
                graph: rinkaku_core::graph::SymbolGraph {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    roots: Vec::new(),
                },
                tests: Vec::new(),
                fan_ins: Vec::new(),
                file_size_warnings: Vec::new(),
                removed: Vec::new(),
            },
            diff_text,
        ));
    }

    let read_file = {
        let head = head.to_string();
        move |path: &str| read_git_show_file(cwd, &head, path)
    };
    // ADR 0014: `--base`/`--pr` mode always knows a base commit, so unlike
    // stdin mode (see `main`'s own `analyze_diff` call), a `read_base_file`
    // port is always supplied here rather than `None` — reusing the same
    // `git show <rev>:<path>` strategy `read_file` already uses for the
    // head side, just pointed at `base` instead. A path that doesn't exist
    // on the base side (e.g. a brand-new file) fails this read, which
    // `analyze_diff` treats as "no base content for this file" rather than
    // an error (see its own doc comment).
    let read_base_file = {
        let base = base.to_string();
        move |path: &str| read_git_show_file(cwd, &base, path)
    };
    let resolver = build_resolver(cli, &diff_text, &read_file, Some(head), cwd, progress)?;
    let changed_paths = changed_paths(&diff_text)?;
    let generated_paths = resolve_generated_paths(cli, &changed_paths, cwd);
    log::debug!("analyzing diff");
    progress.set_phase(AnalysisPhase::AnalyzingDiff);
    // ADR 0033 (amended): reports `(files_done, total)` back through
    // `progress` as `analyze_diff`'s sequential per-file loop works through
    // the diff's changed files — same closure shape as `build_resolver`'s
    // own `on_file_progress` above, since `rinkaku_core::progress::OnProgress`
    // is exactly the `Fn(usize, usize) + Sync` shape `analyze_diff` expects.
    let on_file_progress = |done: usize, total: usize| progress.report_file_progress(done, total);
    let report = analyze_diff(
        &diff_text,
        read_file,
        Some(&read_base_file),
        resolver
            .as_ref()
            .map(|r| r as &dyn rinkaku_core::deps::Resolver),
        // See sibling `analyze_diff` call in `main` for why this negates
        // `exclude_tests` rather than passing it straight through.
        !cli.exclude_tests,
        &generated_paths,
        cli.include_generated,
        Some(&on_file_progress),
    )?;
    if let Some(note) = garbage_input_note(&diff_text, &report) {
        progress.note(note.to_string());
    }
    Ok((report, diff_text))
}

pub(crate) fn changed_paths(diff_text: &str) -> anyhow::Result<Vec<String>> {
    Ok(rinkaku_core::diff::parse_unified_diff(diff_text)?
        .into_iter()
        .map(|changed_file| changed_file.path)
        .collect())
}

pub(crate) fn resolve_generated_paths(
    cli: &Cli,
    changed_paths: &[String],
    cwd: Option<&std::path::Path>,
) -> std::collections::HashSet<String> {
    if cli.include_generated {
        return std::collections::HashSet::new();
    }
    check_generated_paths(cwd, changed_paths)
}

pub(crate) fn build_resolver(
    cli: &Cli,
    diff_text: &str,
    diff_read_file: impl Fn(&str) -> std::io::Result<String>,
    head: Option<&str>,
    cwd: Option<&std::path::Path>,
    progress: &dyn AnalysisProgress,
) -> anyhow::Result<Option<TagsResolver>> {
    if cli.deps == 0 {
        return Ok(None);
    }
    progress.set_phase(AnalysisPhase::BuildingDependencyIndex);

    let reference_names =
        rinkaku_core::pipeline::collect_referenced_names(diff_text, diff_read_file)?;

    let paths = list_git_files(cwd)?;
    log::debug!(
        "building dependency index over {} tracked files",
        paths.len()
    );
    let generated_paths = if cli.include_generated {
        std::collections::HashSet::new()
    } else {
        check_generated_paths_batch(cwd, &paths)
    };
    let files: Vec<(String, String)> = match head {
        // One `git cat-file --batch` child process serves every path
        // (see `read_git_show_files_batch`'s doc comment for why this
        // replaces a `git show` subprocess per file). A single
        // unresolvable path is isolated inside that call (same
        // best-effort skip as the working-tree branch below); the `?`
        // here only ever fires for a genuinely unrecoverable failure
        // (the child process itself failing to start, or the batch
        // stream desyncing), which cannot be isolated to one path.
        Some(head) => read_git_show_files_batch(cwd, head, paths)?,
        None => paths
            .into_iter()
            .filter_map(|path| {
                // A file listed by `git ls-files` can still fail to read
                // (e.g. deleted in the working tree but not yet staged, a
                // submodule gitlink entry) — skipped rather than failing
                // the whole run, since the resolver's index is a
                // best-effort aid, not a correctness-critical input.
                read_working_tree_file(&path)
                    .ok()
                    .map(|content| (path, content))
            })
            .collect(),
    };
    // ADR 0033: reports `(files_done, total)` back through `progress` as
    // `TagsResolver::new`'s sequential indexing loop works through `files`
    // — a plain closure over `progress` (a `&dyn AnalysisProgress`, already
    // object-safe) rather than a new abstraction, since
    // `rinkaku_core::progress::OnProgress` is exactly the `Fn(usize, usize)
    // + Sync` shape `TagsResolver::new` expects.
    let on_file_progress = |done: usize, total: usize| progress.report_file_progress(done, total);
    Ok(Some(TagsResolver::new(
        files,
        language_for_path,
        &reference_names,
        // Same CLI→core polarity flip as the `analyze_diff` /
        // `analyze_repo` calls above (ADR 0025).
        !cli.exclude_tests,
        &generated_paths,
        cli.include_generated,
        Some(&on_file_progress),
    )))
}

pub(crate) fn read_stdin_diff() -> anyhow::Result<String> {
    if std::io::stdin().is_terminal() {
        anyhow::bail!(
            "no diff input: pipe a diff via stdin (e.g. `gh pr diff 123 | rinkaku`) or pass --base <ref>"
        );
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spinner::Spinner;
    use crate::test_util::{init_repo_with_committed_file, run_git};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    // Regression test for the must-fix performance bug: `build_resolver`
    // must return before doing any repository scan when `deps == 0`. This
    // is exercised indirectly rather than by inspecting call counts (no
    // mocking of `git`, per this project's test conventions): `cwd` points
    // at a plain (non-git) tempdir, so if `list_git_files` were reached,
    // `git ls-files` would fail there and `build_resolver` would return
    // `Err`. Observing `Ok(None)` is therefore proof the scan never ran.
    //
    // NOTE: partial assertion (`is_none()` rather than a fully qualified
    // comparison) because `TagsResolver` derives neither `Debug` nor
    // `PartialEq` — its `HashMap` index isn't meant to be compared as a
    // value, only used through `Resolver::resolve`. Which variant of
    // `Option` came back is exactly what this test needs to know.
    #[test]
    fn should_skip_repository_scan_when_deps_is_zero() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        // Never called if `deps == 0` truly short-circuits before doing
        // any work at all — deliberately panics so a regression that
        // starts calling it would fail loudly rather than silently
        // reading an empty string.
        let read_file = |_: &str| -> std::io::Result<String> {
            panic!("read_file must not be called when deps == 0")
        };

        let spinner = Spinner::start("test");
        let actual = build_resolver(&cli, "", read_file, None, Some(dir.path()), &spinner)
            .expect("deps == 0 must not touch the repository at all");

        assert!(actual.is_none());
    }

    // Sibling case to the one above: with `deps == 1` (repository scan
    // enabled), the same non-git `cwd` makes `list_git_files` fail,
    // confirming the scan is actually attempted in this branch and that
    // the `Ok(None)` above is specific to `deps == 0`, not an artifact of
    // the test directory itself.
    #[test]
    fn should_fail_when_deps_is_one_and_cwd_has_no_git_repository() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let read_file = |_: &str| -> std::io::Result<String> { Ok(String::new()) };

        let spinner = Spinner::start("test");
        let actual = build_resolver(&cli, "", read_file, None, Some(dir.path()), &spinner);

        assert!(actual.is_err());
    }

    // Regression test for the must-fix performance/correctness bug: an
    // empty diff (base == head, e.g. `--pr` on an already-merged PR before
    // ADR 0007's fix, or `--base main --head main`) must return the empty
    // `Report` directly, without ever invoking `build_resolver`'s
    // repository-wide `git ls-files` scan. Unlike `deps == 0`'s sibling
    // tests above (which call `build_resolver` directly and can simply
    // point `cwd` at a non-git directory), `run_base_pipeline` calls
    // `run_git_diff` unconditionally first — a non-git `cwd` would make
    // that fail too, before the empty-diff branch is ever reached. So this
    // test instead uses a real repository (required for `run_git_diff` to
    // succeed) and revokes read permission on `.git/index` specifically:
    // `git diff <base>...<head>` (a tree-to-tree comparison between two
    // commits) never opens the index, but `git ls-files` always does — so
    // if `build_resolver` were reached, `list_git_files` would fail and
    // this test would observe `Err` instead of the expected `Ok`.
    #[test]
    fn should_skip_repository_scan_when_diff_is_empty() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo() {}\n");
        let index_path = dir.path().join(".git/index");
        let mut permissions = std::fs::metadata(&index_path)
            .expect("read .git/index metadata")
            .permissions();
        let original_mode = std::os::unix::fs::PermissionsExt::mode(&permissions);
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o000);
        std::fs::set_permissions(&index_path, permissions).expect("revoke .git/index read access");

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let actual = run_base_pipeline(&cli, "HEAD", "HEAD", Some(dir.path()), &spinner);

        // Restore permissions before asserting so a failed assertion
        // doesn't leave an unreadable file behind for the tempdir cleanup.
        let mut permissions = std::fs::metadata(&index_path)
            .expect("re-read .git/index metadata")
            .permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, original_mode);
        std::fs::set_permissions(&index_path, permissions).expect("restore .git/index permissions");

        let (actual_report, _actual_diff_text) =
            actual.expect("empty diff must not touch the repository-wide index scan");
        assert_eq!(
            rinkaku_core::render::Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: Vec::new(),
                skipped: Vec::new(),
                graph: rinkaku_core::graph::SymbolGraph {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    roots: Vec::new(),
                },
                tests: Vec::new(),
                fan_ins: Vec::new(),
                file_size_warnings: Vec::new(),
                removed: Vec::new(),
            },
            actual_report
        );
    }

    // Regression test (companion to garbage_input_note_tests below): a real
    // `--base` run whose diff touches only a test function must produce a
    // Report with a non-empty `tests` summary and empty `files`/`skipped` —
    // the exact shape garbage_input_note must treat as "a legitimate
    // result", not "garbage input". This exercises the run_base_pipeline
    // route end to end (a real git repo, real analyze_diff call), while the
    // garbage_input_note_tests module below exercises the function in
    // isolation with the same report shape; together they cover both the
    // stdin and --base/--pr code paths that call garbage_input_note (both
    // funnel through analyze_diff, so run_base_pipeline's coverage extends
    // to the stdin route too).
    #[test]
    fn should_produce_test_only_report_without_garbage_input_shape_when_diff_touches_only_a_test_under_exclude_tests()
     {
        // ADR 0025 flipped the default to include tests, so the
        // "test-only diff produces empty files + non-empty tests
        // summary" shape this test pins down only occurs under
        // `--exclude-tests`. The regression this guards is still real:
        // garbage_input_note must not flag such a legitimate result as
        // garbage input, and the test-detection wiring must actually
        // populate `Report.tests` when the flag is set.
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(
            dir.path(),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(1, 1 + 0);
}
",
        );
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(2, 1 + 1);
}
",
        )
        .expect("edit src/lib.rs");
        run_git(dir.path(), &["add", "src/lib.rs"]);
        run_git(dir.path(), &["commit", "-m", "fix test assertion"]);

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            exclude_tests: true,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let (actual, _diff_text) =
            run_base_pipeline(&cli, "HEAD~1", "HEAD", Some(dir.path()), &spinner)
                .expect("run_base_pipeline should succeed for a test-only diff");

        let expected_files: Vec<rinkaku_core::render::FileReport> = Vec::new();
        let expected_skipped: Vec<rinkaku_core::render::SkippedFile> = Vec::new();
        assert_eq!(expected_files, actual.files);
        assert_eq!(expected_skipped, actual.skipped);
        assert_eq!(1, actual.tests.len());
        // The Report shape actually produced is the exact input
        // garbage_input_note must not flag — pin that contract down
        // directly here rather than only trusting the isolated unit test.
        assert_eq!(
            None,
            garbage_input_note("dummy non-empty diff text", &actual)
        );
    }

    // Companion to the above under the new default: a test-only diff
    // with `exclude_tests: false` (the ADR 0025 default) should now put
    // the test symbol into `files` like any production symbol, and
    // leave `tests` empty. Pins that the flag actually flips the
    // resulting shape — without this, the previous test would only
    // prove the exclusion branch, and a regression that ignored the
    // flag entirely could pass.
    #[test]
    fn should_include_test_symbol_in_files_when_diff_touches_only_a_test_under_default() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(
            dir.path(),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(1, 1 + 0);
}
",
        );
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "\
#[test]
fn should_add_two_numbers() {
    assert_eq!(2, 1 + 1);
}
",
        )
        .expect("edit src/lib.rs");
        run_git(dir.path(), &["add", "src/lib.rs"]);
        run_git(dir.path(), &["commit", "-m", "fix test assertion"]);

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let (actual, _diff_text) =
            run_base_pipeline(&cli, "HEAD~1", "HEAD", Some(dir.path()), &spinner)
                .expect("run_base_pipeline should succeed for a test-only diff");

        let expected_tests: Vec<rinkaku_core::render::TestFileSummary> = Vec::new();
        assert_eq!(expected_tests, actual.tests);
        assert_eq!(1, actual.files.len());
        assert_eq!(1, actual.files[0].symbols.len());
        assert_eq!(true, actual.files[0].symbols[0].is_test);
    }

    // ADR 0014 end-to-end: `run_base_pipeline` must actually wire a
    // `read_base_file` port backed by `git show <base>:<path>` into
    // `analyze_diff`, so a real `--base`/`--pr` run classifies a
    // signature-changing edit as `signature_changed` — not just that the
    // pure `classify_symbols`/`analyze_diff` functions can do so when fed a
    // base reader directly (already covered by
    // `extract::tests::classification_tests` and
    // `pipeline::tests::classification_wiring_tests`).
    #[test]
    fn should_classify_symbol_as_signature_changed_via_real_base_commit() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo(a: i32) -> i32 {\n    a\n}\n");
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn foo(a: i32, b: i32) -> i32 {\n    a\n}\n",
        )
        .expect("edit src/lib.rs");
        run_git(dir.path(), &["add", "src/lib.rs"]);
        run_git(dir.path(), &["commit", "-m", "widen foo's signature"]);

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let spinner = Spinner::start("test");
        let (actual, _diff_text) =
            run_base_pipeline(&cli, "HEAD~1", "HEAD", Some(dir.path()), &spinner)
                .expect("run_base_pipeline should succeed");

        let symbol = &actual.files[0].symbols[0];
        assert_eq!(
            Some(rinkaku_core::extract::Classification::SignatureChanged),
            symbol.classification
        );
        assert_eq!(
            Some("fn foo(a: i32) -> i32".to_string()),
            symbol.previous_signature
        );
    }

    // resolve_generated_paths takes already-parsed changed paths (a
    // Vec<String>) rather than the raw diff text, so it cannot re-parse the
    // diff itself — parsing happens exactly once at the call site and the
    // resulting paths are shared with analyze_diff's own parse (still
    // unavoidable, since analyze_diff needs the full ChangedFile data, not
    // just paths).
    #[test]
    fn should_resolve_generated_paths_from_already_parsed_changed_paths() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        std::fs::write(dir.path().join(".gitattributes"), "Cargo.lock -diff\n")
            .expect("write .gitattributes");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let changed_paths = vec!["Cargo.lock".to_string()];
        let actual = resolve_generated_paths(&cli, &changed_paths, Some(dir.path()));

        let expected: HashSet<String> = ["Cargo.lock".to_string()].into_iter().collect();
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_return_empty_set_when_include_generated_is_true() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        run_git(dir.path(), &["init", "--initial-branch=main"]);
        std::fs::write(dir.path().join(".gitattributes"), "Cargo.lock -diff\n")
            .expect("write .gitattributes");
        std::fs::write(dir.path().join("Cargo.lock"), "").expect("write Cargo.lock");

        let cli = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: true,
            entry: None,
            tui: false,
        };
        let changed_paths = vec!["Cargo.lock".to_string()];
        let actual = resolve_generated_paths(&cli, &changed_paths, Some(dir.path()));

        let expected: HashSet<String> = HashSet::new();
        assert_eq!(expected, actual);
    }
}
