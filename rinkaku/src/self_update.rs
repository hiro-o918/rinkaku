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
/// This matters beyond UX: the `self_update` crate's confirmation prompt
/// reads a line from stdin and treats an *empty* response as "yes" (see
/// `self_update::confirm`), and reading from a closed/non-TTY stdin
/// immediately yields an empty line (EOF). Left unguarded, running
/// `self-update` with stdin non-interactive (e.g. piped, `/dev/null`, or a
/// CI job) would auto-confirm the update. This function is checked before
/// ever calling into `self_update`, so that case is refused explicitly
/// instead. It also means an accidental `rinkaku self-update` (e.g. a typo
/// of `--base main`, see PR review) can no longer silently replace the
/// binary when stdin isn't a terminal.
fn confirm_mode(yes: bool, stdin_is_tty: bool) -> ConfirmMode {
    if yes {
        ConfirmMode::Skip
    } else if stdin_is_tty {
        ConfirmMode::Prompt
    } else {
        ConfirmMode::RefuseNonInteractive
    }
}

/// Runs `self-update`: downloads and installs the latest GitHub release
/// asset matching the running binary's target triple, replacing the
/// current executable in place.
///
/// `yes` corresponds to the `--yes`/`-y` flag; TTY detection (the other
/// input to `confirm_mode`) is read here at the composition-root boundary
/// rather than passed in, since it is itself a form of environment IO.
pub fn run_self_update(yes: bool) -> Result<()> {
    let no_confirm = match confirm_mode(yes, std::io::stdin().is_terminal()) {
        ConfirmMode::Skip => true,
        ConfirmMode::Prompt => false,
        ConfirmMode::RefuseNonInteractive => {
            anyhow::bail!("refusing to self-update non-interactively; pass --yes to proceed");
        }
    };

    let current_version = env!("CARGO_PKG_VERSION");
    let target = self_update::get_target();

    let status = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .target(target)
        .current_version(current_version)
        .identifier(&release_asset_name(BIN_NAME, target))
        .bin_path_in_archive(&archive_bin_path(BIN_NAME, target))
        .no_confirm(no_confirm)
        .build()?
        .update()
        .context(
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
    // archive path/no_confirm) without error.
    #[test]
    fn should_configure_update_builder_without_error() {
        let target = self_update::get_target();
        let current_version = env!("CARGO_PKG_VERSION");

        // Verifies the builder itself accepts this configuration (no
        // network IO — `.build()` only validates configuration, `.update()`
        // is what would actually hit the network and is intentionally not
        // exercised here per this project's "no mocking external
        // processes" test convention).
        let result = self_update::backends::github::Update::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(BIN_NAME)
            .target(target)
            .current_version(current_version)
            .identifier(&release_asset_name(BIN_NAME, target))
            .bin_path_in_archive(&archive_bin_path(BIN_NAME, target))
            .no_confirm(true)
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
}
