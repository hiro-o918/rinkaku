//! Opens a URL in the reviewer's default web browser (ADR 0049) by spawning
//! the platform's own "open a URL" command (`open` on macOS, `xdg-open` on
//! Linux) — mirroring `clipboard.rs`'s direct-spawn shape rather than adding
//! a crate dependency for one OS command invocation.

use rinkaku_tui::review::ports::BrowserOpener;

pub(crate) struct SystemBrowserOpener;

/// The command used to open a URL, by target OS — `cfg!(target_os = ...)`
/// rather than `#[cfg(...)]` items, so [`command_for_os`] stays a plain,
/// unit-testable function instead of three mutually-exclusive compiled
/// variants (matching this crate's existing "extract the pure choice,
/// spawn separately" shape, `clipboard.rs`'s `choose_clipboard_backend`).
fn command_for_os(target_os: &str) -> Option<&'static str> {
    match target_os {
        "macos" => Some("open"),
        "linux" => Some("xdg-open"),
        _ => None,
    }
}

/// Spawns `program url` and waits for it to exit — split out of
/// [`BrowserOpener::open_url`] so a test can exercise the spawn-failure path
/// against a nonexistent program name without depending on which command
/// [`command_for_os`] would have chosen on the host platform (mirroring
/// `clipboard.rs`'s own `copy_via_command` extraction).
fn open_via_command(program: &str, url: &str) -> Result<(), String> {
    let status = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to spawn {program}: {err}"))?
        .wait()
        .map_err(|err| format!("failed to wait for {program}: {err}"))?;
    if !status.success() {
        return Err(format!("{program} exited with {status}"));
    }
    Ok(())
}

impl BrowserOpener for SystemBrowserOpener {
    fn open_url(&self, url: &str) -> Result<(), String> {
        let Some(program) = command_for_os(std::env::consts::OS) else {
            return Err(format!(
                "no known browser-open command for platform {}",
                std::env::consts::OS
            ));
        };
        open_via_command(program, url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_choose_open_on_macos() {
        let actual = command_for_os("macos");

        assert_eq!(Some("open"), actual);
    }

    #[test]
    fn should_choose_xdg_open_on_linux() {
        let actual = command_for_os("linux");

        assert_eq!(Some("xdg-open"), actual);
    }

    #[test]
    fn should_return_none_for_an_unsupported_platform() {
        let actual = command_for_os("windows");

        assert_eq!(None, actual);
    }

    #[test]
    fn should_report_spawn_failure_when_command_does_not_exist() {
        let actual = open_via_command("rinkaku-nonexistent-browser-cmd", "https://example.com");

        assert_eq!(
            true,
            actual
                .unwrap_err()
                .starts_with("failed to spawn rinkaku-nonexistent-browser-cmd")
        );
    }
}
