//! `rinkaku self-update`: replaces the running binary with the latest
//! GitHub release, following the same `self_update` crate setup as
//! `hiro-o918/skem`'s `src/self_update.rs` (same crate version, feature
//! set, and `Update::configure()` shape) for consistency across this
//! author's CLIs.
//!
//! Kept in the `rinkaku` bin crate rather than `rinkaku-core`: this is
//! process/network IO tied to how *this specific binary* is distributed
//! (GitHub Releases asset naming), not part of the pure diff-condensation
//! core.
//!
//! See README's Release section for how `rinkaku` and `rinkaku-core` are
//! versioned and tagged independently.

use anyhow::{Context, Result};
use std::io::IsTerminal;

const REPO_OWNER: &str = "hiro-o918";
const REPO_NAME: &str = "rinkaku";
const BIN_NAME: &str = "rinkaku";

/// How the update prompt should behave, decided up front from `--yes` and
/// whether stdin is a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmMode {
    /// Skip the confirmation prompt (`--yes` was passed).
    Skip,
    /// Show the interactive confirmation prompt (stdin is a TTY).
    Prompt,
    /// Refuse to update: `--yes` was not passed and stdin is not a TTY, so
    /// there is no one to answer the prompt.
    RefuseNonInteractive,
}

/// Decides how `self-update` should confirm before installing, without
/// touching the terminal or the network — kept pure so the three cases are
/// unit-testable directly.
///
/// This matters beyond UX, for two independent reasons:
///
/// - Confirmation is now handled entirely by our own code (`is_affirmative`
///   below), not the `self_update` crate's `confirm()` — the crate is
///   always run with `no_confirm(true)` so it never prompts on its own
///   (see `run_self_update`'s doc comment for why). `RefuseNonInteractive`
///   is what actually blocks running non-interactively.
/// - Even with our own prompt, reading from a closed/non-TTY stdin
///   immediately yields an empty line (EOF), and `is_affirmative` treats
///   empty as *not* affirmative (deliberately the opposite of the crate's
///   own `confirm()`, which treats empty as "yes" — see its doc comment).
///   `RefuseNonInteractive` still exists as a defense-in-depth guard
///   before that: it fails fast with a clear message instead of silently
///   reading an empty line and declining, and it means an accidental
///   `rinkaku self-update` (e.g. a typo of `--base main`, see PR review)
///   can no longer be interpreted as any kind of answer when stdin isn't a
///   terminal.
fn confirm_mode(yes: bool, stdin_is_tty: bool) -> ConfirmMode {
    if yes {
        ConfirmMode::Skip
    } else if stdin_is_tty {
        ConfirmMode::Prompt
    } else {
        ConfirmMode::RefuseNonInteractive
    }
}

/// Whether a confirmation answer counts as "yes": trimmed and compared
/// case-insensitively against `y` / `yes`. Everything else — including an
/// empty string — is not affirmative.
///
/// This is the deliberate opposite of the `self_update` crate's own
/// `confirm()` helper, which treats an *empty* response as "yes" (see
/// `confirm_mode`'s doc comment for why that default is unsafe for us):
/// requiring an explicit `y`/`yes` means a stray newline or a
/// misunderstood prompt can never be read as consent to replace the
/// running binary.
fn is_affirmative(answer: &str) -> bool {
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Runs `self-update`: downloads and installs the latest GitHub release
/// asset matching the running binary's target triple, replacing the
/// current executable in place.
///
/// All user-facing messaging is owned by this function rather than the
/// `self_update` crate's own `println!`s, which is why the builder below
/// always sets `.no_confirm(true)` and `.show_output(false)`: reading
/// `self_update` v0.42's `update_extended()` (`src/update.rs`) shows every
/// line it would otherwise print — including `Checking target-arch...`,
/// `New release found! v{cur} --> v{new}`, and, notably, `New release is
/// {*NOT* }compatible` — is gated behind `show_output` (via its internal
/// `println`/`print_flush` helpers), and the `"{bin} release status:"`
/// block plus its own confirmation prompt are gated behind
/// `show_output() || !no_confirm()`, i.e. `show_output(false)` +
/// `no_confirm(true)` together skip both. The `*NOT* compatible` line in
/// particular is actively misleading for us: it comes from
/// `self_update::version::bump_is_compatible`, which treats *any* 0.x
/// minor bump (e.g. 0.3.x -> 0.4.0) as incompatible by semver convention —
/// correct for that crate's general-purpose semantics, wrong for a
/// pre-1.0 tool whose 0.x minor bumps are just ordinary releases.
/// `.show_download_progress(true)` is kept on: the download progress bar
/// is rendered by `indicatif` directly (`Download::download_to`), not by
/// any of the suppressed `println!`s, so it stays as a real progress
/// indicator with none of the chatter.
///
/// `yes` corresponds to the `--yes`/`-y` flag; TTY detection (the other
/// input to `confirm_mode`) is read here at the composition-root boundary
/// rather than passed in, since it is itself a form of environment IO.
pub fn run_self_update(yes: bool) -> Result<()> {
    let confirm_mode = confirm_mode(yes, std::io::stdin().is_terminal());
    if confirm_mode == ConfirmMode::RefuseNonInteractive {
        anyhow::bail!("refusing to self-update non-interactively; pass --yes to proceed");
    }

    let current_version = env!("CARGO_PKG_VERSION");
    let target = self_update::get_target();

    let updater = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .target(target)
        .current_version(current_version)
        .identifier(&release_asset_name(BIN_NAME, target))
        .bin_path_in_archive(&archive_bin_path(BIN_NAME, target))
        .no_confirm(true)
        .show_output(false)
        .show_download_progress(true)
        .build()?;

    let latest = updater
        .get_latest_release()
        .context("failed to check the latest rinkaku release")?;
    let is_newer = self_update::version::bump_is_greater(current_version, &latest.version)
        .with_context(|| {
            format!(
                "failed to compare current version v{current_version} with latest v{}",
                latest.version
            )
        })?;
    if !is_newer {
        println!("{BIN_NAME} is already up to date (v{current_version})");
        return Ok(());
    }

    println!(
        "New release found: v{current_version} -> v{}",
        latest.version
    );

    if confirm_mode == ConfirmMode::Prompt {
        print!("Update to v{}? [y/N]: ", latest.version);
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !is_affirmative(&answer) {
            println!("Update cancelled");
            return Ok(());
        }
    }

    let status = updater.update().context(
        "self-update failed; if this is a permission error, try again with sudo, \
         or update via the package manager you installed rinkaku with \
         (e.g. `brew upgrade` or `cargo install rinkaku`)",
    )?;

    if status.updated() {
        println!(
            "Updated {BIN_NAME} from v{current_version} to {}",
            status.version()
        );
    } else {
        println!("{BIN_NAME} is already up to date (v{current_version})");
    }

    Ok(())
}

/// Builds the release asset filename for `bin` on `target`, matching the
/// `build-and-publish.yaml` packaging convention:
/// `{bin}-{target}.tar.gz` (e.g. `rinkaku-aarch64-apple-darwin.tar.gz`).
///
/// Passed as `Update::configure()`'s `identifier`, which the `self_update`
/// crate matches against release asset names via a `contains` substring
/// check, not an exact match. Passing the full asset filename (rather than
/// just the target triple) is deliberate: it narrows that substring check
/// down to what is, in practice, an exact match against a single release
/// asset, so a future asset whose name happens to contain this target
/// triple as a substring (e.g. a differently-suffixed archive for the same
/// triple) cannot be picked up by mistake.
fn release_asset_name(bin: &str, target: &str) -> String {
    format!("{bin}-{target}.tar.gz")
}

/// Builds the path to the binary inside the release archive. The archive
/// contains a top-level `{bin}-{target}/` directory (see
/// `build-and-publish.yaml`'s "Package binary" step), so the binary lives
/// one level down from the archive root rather than at its root.
fn archive_bin_path(bin: &str, target: &str) -> String {
    format!("{bin}-{target}/{bin}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[test]
    fn should_build_dot_tar_gz_asset_name_from_bin_and_target() {
        let actual = release_asset_name("rinkaku", "aarch64-apple-darwin");

        assert_eq!("rinkaku-aarch64-apple-darwin.tar.gz", actual);
    }

    #[test]
    fn should_build_nested_archive_path_from_bin_and_target() {
        let actual = archive_bin_path("rinkaku", "x86_64-unknown-linux-gnu");

        assert_eq!("rinkaku-x86_64-unknown-linux-gnu/rinkaku", actual);
    }

    // NOTE: partial assertion (`is_ok()` rather than a fully qualified
    // comparison of the built `Update`) because `self_update::backends::
    // github::Update`'s fields are private and it derives neither `Debug`
    // nor `PartialEq` — there is nothing to compare it against. `is_ok()`
    // is the strongest assertion available: it proves the builder accepts
    // this exact configuration (owner/repo/bin/target/version/identifier/
    // archive path/no_confirm/show_output/show_download_progress) without
    // error.
    #[test]
    fn should_configure_update_builder_without_error() {
        let target = self_update::get_target();
        let current_version = env!("CARGO_PKG_VERSION");

        // Verifies the builder itself accepts this configuration (no
        // network IO — `.build()` only validates configuration,
        // `.get_latest_release()`/`.update()` are what would actually hit
        // the network and are intentionally not exercised here per this
        // project's "no mocking external processes" test convention).
        // `no_confirm(true)` and `show_output(false)` are always-on now
        // (see `run_self_update`'s doc comment for why), so this is the
        // one configuration the builder is ever actually built with.
        let result = self_update::backends::github::Update::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(BIN_NAME)
            .target(target)
            .current_version(current_version)
            .identifier(&release_asset_name(BIN_NAME, target))
            .bin_path_in_archive(&archive_bin_path(BIN_NAME, target))
            .no_confirm(true)
            .show_output(false)
            .show_download_progress(true)
            .build();

        assert!(
            result.is_ok(),
            "Update builder configuration should succeed"
        );
    }

    #[test]
    fn should_skip_confirmation_when_yes_flag_is_set() {
        let actual = confirm_mode(true, false);

        assert_eq!(ConfirmMode::Skip, actual);
    }

    #[test]
    fn should_prompt_when_yes_flag_is_unset_and_stdin_is_a_tty() {
        let actual = confirm_mode(false, true);

        assert_eq!(ConfirmMode::Prompt, actual);
    }

    #[test]
    fn should_refuse_non_interactively_when_yes_flag_is_unset_and_stdin_is_not_a_tty() {
        let actual = confirm_mode(false, false);

        assert_eq!(ConfirmMode::RefuseNonInteractive, actual);
    }

    #[test]
    fn should_skip_confirmation_when_yes_flag_is_set_even_with_a_tty() {
        // `--yes` always wins regardless of TTY-ness.
        let actual = confirm_mode(true, true);

        assert_eq!(ConfirmMode::Skip, actual);
    }

    #[rstest]
    #[case::should_accept_lowercase_y("y", true)]
    #[case::should_accept_uppercase_y("Y", true)]
    #[case::should_accept_lowercase_yes("yes", true)]
    #[case::should_accept_uppercase_yes("YES", true)]
    #[case::should_accept_mixed_case_yes("Yes", true)]
    #[case::should_accept_y_with_surrounding_whitespace("  y  \n", true)]
    #[case::should_accept_yes_with_surrounding_whitespace("  yes  \n", true)]
    #[case::should_reject_empty_string("", false)]
    #[case::should_reject_whitespace_only_string("   \n", false)]
    #[case::should_reject_n("n", false)]
    #[case::should_reject_no("no", false)]
    #[case::should_reject_arbitrary_text("sure why not", false)]
    #[case::should_reject_y_as_prefix_of_longer_word("yesterday", false)]
    fn should_check_is_affirmative(#[case] answer: &str, #[case] expected: bool) {
        let actual = is_affirmative(answer);

        assert_eq!(expected, actual);
    }
}
