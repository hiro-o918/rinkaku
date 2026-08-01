use super::super::*;
use pretty_assertions::assert_eq;
use rstest::rstest;

#[rstest]
#[case::should_detect_lock_file_at_repo_root(".terraform.lock.hcl", true)]
#[case::should_detect_lock_file_in_subdirectory("envs/prod/.terraform.lock.hcl", true)]
#[case::should_not_detect_ordinary_hcl_file("config.hcl", false)]
#[case::should_not_detect_suffix_without_dot_directory("x.terraform.lock.hcl", false)]
fn is_generated_lockfile_path_cases(#[case] path: &str, #[case] expected: bool) {
    let actual = is_generated_lockfile_path(path);

    assert_eq!(expected, actual);
}
