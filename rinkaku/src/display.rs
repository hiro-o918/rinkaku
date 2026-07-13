//! Display mode resolution extracted from `main.rs`.

use crate::cli::Format;

/// Which output stage `main` dispatches to, once a `Report` is built —
/// pulled into its own type (rather than inlining the `if cli.tui`/
/// `render` branch as before) so the *decision* of which one to use can be
/// unit-tested as a pure function ([`resolve_display_mode`]) independent
/// of actually running the TUI or rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisplayMode {
    Tui,
    Output(Format),
}

/// Decides which [`DisplayMode`] to use from the three inputs that can
/// influence it: whether `--tui` was passed, whether `--format` was passed
/// (`Some` — clap's `conflicts_with` already guarantees `tui` and `format`
/// are never both meaningfully set, see `Cli::format`'s doc comment), and
/// whether stdout is a terminal.
///
/// - `--tui` passed → [`DisplayMode::Tui`], regardless of stdout.
/// - `--format` passed (and `--tui` wasn't, by the conflict above) →
///   [`DisplayMode::Output`] with that format — an explicit format request
///   always wins, whether or not stdout happens to be a terminal (this is
///   what lets a non-interactive caller force whole-repo mode's Markdown
///   output even while attached to a terminal, e.g. `rinkaku --format md
///   > out.md` run interactively, or this project's own dogfooding
///   `rinkaku --format md` invocations in CI-like scripts).
/// - Neither passed → ADR 0017's default: [`DisplayMode::Tui`] when stdout
///   is a terminal (a human is watching, so they get the interactive
///   view — ADR 0015), [`DisplayMode::Output(Format::Md)`] otherwise (a
///   pipe/redirect, so Markdown is what a non-interactive consumer can
///   actually use).
///
/// Pure and total over its three `bool`/`Option` inputs — no `IsTerminal`
/// call here, `main` reads the real streams and passes the results in.
pub(crate) fn resolve_display_mode(
    tui: bool,
    format: Option<Format>,
    stdout_is_tty: bool,
) -> DisplayMode {
    if tui {
        return DisplayMode::Tui;
    }
    if let Some(format) = format {
        return DisplayMode::Output(format);
    }
    if stdout_is_tty {
        DisplayMode::Tui
    } else {
        DisplayMode::Output(Format::Md)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_choose_tui_when_tui_flag_is_set_and_stdout_is_a_terminal(
        true,
        None,
        true,
        DisplayMode::Tui
    )]
    #[case::should_choose_tui_when_tui_flag_is_set_and_stdout_is_not_a_terminal(
        true,
        None,
        false,
        DisplayMode::Tui
    )]
    #[case::should_choose_explicit_format_over_terminal_stdout(
        false,
        Some(Format::Json),
        true,
        DisplayMode::Output(Format::Json)
    )]
    #[case::should_choose_explicit_format_over_non_terminal_stdout(
        false,
        Some(Format::Md),
        false,
        DisplayMode::Output(Format::Md)
    )]
    #[case::should_default_to_tui_when_neither_flag_is_set_and_stdout_is_a_terminal(
        false,
        None,
        true,
        DisplayMode::Tui
    )]
    #[case::should_default_to_markdown_when_neither_flag_is_set_and_stdout_is_not_a_terminal(
        false,
        None,
        false,
        DisplayMode::Output(Format::Md)
    )]
    fn resolve_display_mode_cases(
        #[case] tui: bool,
        #[case] format: Option<Format>,
        #[case] stdout_is_tty: bool,
        #[case] expected: DisplayMode,
    ) {
        let actual = resolve_display_mode(tui, format, stdout_is_tty);

        assert_eq!(expected, actual);
    }
}
