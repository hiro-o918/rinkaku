//! Clipboard export (ADR 0048 sink B). A native clipboard command
//! (`pbcopy`/`wl-copy`/`xclip`/`xsel`) is preferred when the session is
//! local, because OSC 52 is silently dropped by common local setups (tmux
//! `set-clipboard external` forwarding to an outer terminal that doesn't
//! support or permit it); OSC 52 remains the fallback for remote (SSH)
//! sessions — the environment it was chosen for — and for hosts with no
//! clipboard command at all. See ADR 0048's amendment for the rationale.

use rinkaku_tui::review::ports::ClipboardSink;
use std::io::Write;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClipboardBackend {
    Command {
        program: &'static str,
        args: &'static [&'static str],
    },
    Osc52,
}

const COMMAND_CANDIDATES: &[(&str, &[&str])] = &[
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
];

pub(crate) fn choose_clipboard_backend(
    remote: bool,
    available: impl Fn(&str) -> bool,
) -> ClipboardBackend {
    if remote {
        return ClipboardBackend::Osc52;
    }
    COMMAND_CANDIDATES
        .iter()
        .find(|(program, _)| available(program))
        .map(|&(program, args)| ClipboardBackend::Command { program, args })
        .unwrap_or(ClipboardBackend::Osc52)
}

pub(crate) struct SystemClipboard {
    backend: ClipboardBackend,
}

impl SystemClipboard {
    pub(crate) fn detect() -> Self {
        let remote =
            std::env::var_os("SSH_TTY").is_some() || std::env::var_os("SSH_CONNECTION").is_some();
        Self {
            backend: choose_clipboard_backend(remote, is_in_path),
        }
    }
}

impl ClipboardSink for SystemClipboard {
    fn copy(&self, text: &str) -> Result<String, String> {
        match self.backend {
            ClipboardBackend::Command { program, args } => copy_via_command(program, args, text),
            ClipboardBackend::Osc52 => copy_via_osc52(text),
        }
    }
}

fn is_in_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

fn copy_via_command(program: &str, args: &[&str], text: &str) -> Result<String, String> {
    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to spawn {program}: {err}"))?;
    if let Some(stdin) = child.stdin.take() {
        let mut stdin = stdin;
        stdin
            .write_all(text.as_bytes())
            .map_err(|err| format!("failed to write to {program}: {err}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("failed to wait for {program}: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(format!("copied review notes to clipboard via {program}"))
}

/// A conservative guard on the raw packet length, below common
/// terminal-side OSC 52 payload caps (~100KB is a frequently cited limit)
/// even after base64 inflates the wire payload to ~4/3 — terminals that
/// enforce such a cap silently drop or truncate rather than erroring, so
/// the write itself cannot detect the failure; the guard only changes the
/// status message.
const OSC52_SIZE_GUARD_BYTES: usize = 48 * 1024;

fn copy_via_osc52(text: &str) -> Result<String, String> {
    let encoded = base64_encode(text.as_bytes());
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    let mut stdout = std::io::stdout();
    stdout
        .write_all(sequence.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|err| format!("failed to write OSC 52 sequence: {err}"))?;
    let base = "copied review notes to clipboard via OSC 52 (terminal support required)";
    if text.len() > OSC52_SIZE_GUARD_BYTES {
        Ok(format!(
            "{base} — packet is {} bytes, which may exceed the terminal's OSC 52 limit; \
             copy manually if the paste looks truncated",
            text.len()
        ))
    } else {
        Ok(base.to_string())
    }
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 encoding (RFC 4648, with `=` padding) — implemented
/// directly rather than adding a dependency for one small, stable
/// algorithm (ADR 0048's own "no new dependency" rationale for OSC 52
/// applies equally here).
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied();
        let b2 = chunk.get(2).copied();

        out.push(BASE64_ALPHABET[(b0 >> 2) as usize] as char);
        out.push(
            BASE64_ALPHABET[(((b0 & 0b0000_0011) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char,
        );
        match b1 {
            Some(b1) => {
                out.push(
                    BASE64_ALPHABET[(((b1 & 0b0000_1111) << 2) | (b2.unwrap_or(0) >> 6)) as usize]
                        as char,
                );
            }
            None => out.push('='),
        }
        match b2 {
            Some(b2) => out.push(BASE64_ALPHABET[(b2 & 0b0011_1111) as usize] as char),
            None => out.push('='),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_choose_osc52_when_session_is_remote_even_if_commands_exist() {
        let actual = choose_clipboard_backend(true, |_| true);

        assert_eq!(ClipboardBackend::Osc52, actual);
    }

    #[test]
    fn should_choose_pbcopy_when_local_and_pbcopy_is_available() {
        let actual = choose_clipboard_backend(false, |program| program == "pbcopy");

        assert_eq!(
            ClipboardBackend::Command {
                program: "pbcopy",
                args: &[],
            },
            actual
        );
    }

    #[test]
    fn should_choose_xclip_with_selection_args_when_only_xclip_is_available() {
        let actual = choose_clipboard_backend(false, |program| program == "xclip");

        assert_eq!(
            ClipboardBackend::Command {
                program: "xclip",
                args: &["-selection", "clipboard"],
            },
            actual
        );
    }

    #[test]
    fn should_fall_back_to_osc52_when_local_but_no_command_is_available() {
        let actual = choose_clipboard_backend(false, |_| false);

        assert_eq!(ClipboardBackend::Osc52, actual);
    }

    #[test]
    fn should_report_spawn_failure_when_command_does_not_exist() {
        let actual = copy_via_command("rinkaku-nonexistent-clipboard-cmd", &[], "text");

        assert_eq!(
            true,
            actual
                .unwrap_err()
                .starts_with("failed to spawn rinkaku-nonexistent-clipboard-cmd")
        );
    }

    #[test]
    fn should_encode_empty_input_as_empty_string() {
        let actual = base64_encode(b"");

        assert_eq!("", actual);
    }

    #[test]
    fn should_encode_known_vector_with_no_padding() {
        // "Man" -> "TWFu" is the canonical RFC 4648 base64 example.
        let actual = base64_encode(b"Man");

        assert_eq!("TWFu", actual);
    }

    #[test]
    fn should_encode_known_vector_with_one_padding_character() {
        let actual = base64_encode(b"Ma");

        assert_eq!("TWE=", actual);
    }

    #[test]
    fn should_encode_known_vector_with_two_padding_characters() {
        let actual = base64_encode(b"M");

        assert_eq!("TQ==", actual);
    }

    #[test]
    fn should_encode_the_full_rfc_4648_test_vector() {
        let actual = base64_encode(b"pleasure.");

        assert_eq!("cGxlYXN1cmUu", actual);
    }
}
