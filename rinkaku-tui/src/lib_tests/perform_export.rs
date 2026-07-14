//! `clipboard_export_status` tests (ADR 0048): folding sink B's `Result`
//! into a status message, including the OSC 52 size-guard warning for a
//! packet that risks exceeding a terminal's payload limit.

use crate::{OSC52_SIZE_GUARD_BYTES, clipboard_export_status};

#[test]
fn should_report_plain_success_when_packet_is_small() {
    let actual = clipboard_export_status("small packet", Ok(()));

    assert_eq!(
        "copied review notes to clipboard via OSC 52 (terminal support required)",
        actual
    );
}

#[test]
fn should_warn_about_the_osc_52_limit_when_packet_exceeds_the_size_guard() {
    // A conservative terminal-side OSC 52 payload cap is commonly cited
    // around 100KB; base64 inflates the wire payload to ~4/3 of the raw
    // packet, so this crate's own guard sits below that on the raw side.
    let packet = "x".repeat(OSC52_SIZE_GUARD_BYTES + 1);

    let actual = clipboard_export_status(&packet, Ok(()));

    assert!(actual.contains("OSC 52"));
    assert!(actual.contains("manual"));
}

#[test]
fn should_not_warn_when_packet_is_exactly_at_the_size_guard() {
    let packet = "x".repeat(OSC52_SIZE_GUARD_BYTES);

    let actual = clipboard_export_status(&packet, Ok(()));

    assert!(!actual.contains("OSC 52 limit"));
}

#[test]
fn should_report_the_clipboard_ports_error_message_when_copy_fails() {
    let actual = clipboard_export_status("small packet", Err("no tty".to_string()));

    assert_eq!("error copying to clipboard: no tty", actual);
}

#[test]
fn should_report_the_clipboard_ports_error_message_even_when_packet_is_oversized() {
    // The size guard is advisory only, added to a *successful* copy's
    // status — an error from the port itself must still be reported
    // as-is, not overridden by the oversized-packet warning.
    let packet = "x".repeat(OSC52_SIZE_GUARD_BYTES + 1);

    let actual = clipboard_export_status(&packet, Err("no tty".to_string()));

    assert_eq!("error copying to clipboard: no tty", actual);
}
