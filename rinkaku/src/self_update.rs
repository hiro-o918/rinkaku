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

use anyhow::Result;

const REPO_OWNER: &str = "hiro-o918";
const REPO_NAME: &str = "rinkaku";
const BIN_NAME: &str = "rinkaku";

/// Runs `self-update`: downloads and installs the latest GitHub release
/// asset matching the running binary's target triple, replacing the
/// current executable in place.
pub fn run_self_update() -> Result<()> {
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
        .build()?
        .update()?;

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
            .build();

        assert!(
            result.is_ok(),
            "Update builder configuration should succeed"
        );
    }
}
