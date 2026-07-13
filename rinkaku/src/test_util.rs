//! Shared test-only helpers for the split modules. Everything here is
//! `#[cfg(test)]`, so it never ships in the production binary.

/// Runs `git` inside `dir`, panicking with the captured stderr on failure.
/// Test-only helper: production code never wants a panicking git wrapper.
pub(crate) fn run_git(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git must be installed to run this test");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Sets up a throwaway git repository with deterministic author/committer
/// identity (avoids depending on the host's global git config) and one
/// commit containing `src/lib.rs` with `content`.
pub(crate) fn init_repo_with_committed_file(dir: &std::path::Path, content: &str) {
    run_git(dir, &["init", "--initial-branch=main"]);
    run_git(dir, &["config", "user.email", "test@example.com"]);
    run_git(dir, &["config", "user.name", "Test"]);
    std::fs::create_dir_all(dir.join("src")).expect("create src dir");
    std::fs::write(dir.join("src/lib.rs"), content).expect("write src/lib.rs");
    run_git(dir, &["add", "src/lib.rs"]);
    run_git(dir, &["commit", "-m", "initial commit"]);
}
