//! CLI argument definitions extracted from `main.rs`.

use clap::{Parser, Subcommand};
use rinkaku_core::render::OutputFormat;

/// rinkaku (輪郭) — condense PR diffs into signatures and their dependencies.
#[derive(Parser, Debug, PartialEq, Eq)]
#[command(name = "rinkaku", version, about, long_about = None)]
pub(crate) struct Cli {
    /// Subcommand to run. Omitted for the default diff-condensation flow
    /// (stdin / `--base` / `--deps` / `--format` below), which stays the
    /// primary, backward-compatible entry point.
    #[command(subcommand)]
    pub(crate) command: Option<Command>,

    /// Base ref to diff against (runs `git diff <base>...<head>` instead
    /// of reading from stdin).
    #[arg(long, conflicts_with = "pr")]
    pub(crate) base: Option<String>,

    /// Head ref to diff against `base`. Only meaningful together with
    /// `--base`; defaults to `HEAD`.
    //
    // `conflicts_with = "pr"` only fires when `--head` is explicitly
    // passed (clap does not treat a default value as "provided"), which
    // is exactly what's wanted: `--pr` resolves its own head commit via
    // `gh`, so an explicit `--head` alongside `--pr` would be silently
    // ignored otherwise.
    #[arg(long, default_value = "HEAD", conflicts_with = "pr")]
    pub(crate) head: String,

    /// GitHub PR to review, as a URL
    /// (`https://github.com/<owner>/<repo>/pull/<number>`) or a bare PR
    /// number (`76`). A bare number must be run inside a local clone of
    /// the target repository; a URL also works from any other directory
    /// by auto-cloning into a cache. Requires `gh` installed and
    /// authenticated.
    // See ADR 0004 for the resolve-then-fetch design and ADR 0005 for the
    // auto-clone-into-cache behavior this drives in `main`.
    #[arg(long)]
    pub(crate) pr: Option<String>,

    /// Output format. Defaults to Markdown, or the interactive TUI when
    /// stdout is a terminal and neither `--format` nor `--tui` was given.
    //
    // See `resolve_display_mode` (ADR 0017) for how the default is picked.
    //
    // `Option` rather than a `default_value_t` is what makes "the user
    // didn't pass --format" observable at all; a defaulted `Format` field
    // would look identical to an explicit `--format md`, which
    // `resolve_display_mode` needs to tell apart (see its own doc comment).
    #[arg(long, value_enum, conflicts_with = "tui")]
    pub(crate) format: Option<Format>,

    /// Open the interactive terminal UI instead of printing Markdown/JSON.
    /// The input flow (stdin / `--base` / `--pr`) is unchanged — `--tui`
    /// only changes the output stage, once a `Report` is built. Conflicts
    /// with `--format`, since the two are mutually exclusive output stages
    /// rather than combinable options.
    // See ADR 0015/0016 for the design behind the TUI itself.
    #[arg(long, default_value_t = false)]
    pub(crate) tui: bool,

    /// Whether to resolve each changed symbol's 1-hop dependencies. `1`
    /// (default) runs the tags-based `Resolver` over every file tracked by
    /// `git ls-files`; `0` skips resolution entirely (no
    /// `Resolver::resolve` calls), which is faster and avoids the
    /// repo-wide indexing pass.
    // See ADR 0003 for the 1-hop dependency resolution design.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=1))]
    pub(crate) deps: u8,

    /// Exclude test symbols from the "Change graph"/"Definitions" output
    /// and summarize their per-file counts under a "Tests" section
    /// instead. Without this flag, test symbols appear in the graph and
    /// definitions like any other symbol — the default the Markdown/JSON
    /// output is designed around now that its primary audience is LLM
    /// reviewers (humans read the TUI, which badges test files rather than
    /// omitting them).
    // See ADR 0025 (superseding the ADR 0009 default) for the rationale
    // behind this default.
    #[arg(long, default_value_t = false)]
    pub(crate) exclude_tests: bool,

    /// Include files `.gitattributes` marks `-diff` or `linguist-generated`
    /// instead of skipping them by default.
    // See ADR 0010 for why generated files are skipped by default.
    #[arg(long, default_value_t = false)]
    pub(crate) include_generated: bool,

    /// Re-root the change graph at this path before rendering: entry
    /// points become the symbols under `path` that nothing else under
    /// that same path depends on, and dependency trees still expand
    /// outward through the full graph as usual. This is a viewpoint
    /// change, not a filter — symbols outside `path` are neither hidden
    /// nor excluded from analysis, only no longer eligible to be roots
    /// themselves. Compatible with every input mode (stdin/`--base`/`--pr`/
    /// whole-repo) and with `--tui`: combined, the TUI opens with the
    /// cursor already on the tree row matching `path` and the right pane
    /// already showing its Blast radius, rather than requiring the
    /// reviewer to find the row and press `R` themselves.
    // See ADR 0019 for the re-rooting design and ADR 0023 for the
    // `rinkaku_tui::run` `entry_path` parameter this drives.
    #[arg(long)]
    pub(crate) entry: Option<String>,
}
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Update rinkaku to the latest GitHub release in place. If you
    /// installed via Homebrew or `cargo install`, prefer `brew upgrade`
    /// or `cargo install rinkaku` instead so your package manager stays
    /// in sync — self-update works either way, but it bypasses those
    /// managers' bookkeeping.
    ///
    /// Requires either an interactive terminal (to confirm the update) or
    /// `--yes`. Refuses to run when stdin is not a TTY and `--yes` is not
    /// given, since there would be no one to answer the confirmation
    /// prompt.
    SelfUpdate {
        /// Skip the interactive confirmation prompt and proceed.
        #[arg(long, short = 'y')]
        yes: bool,
    },
}
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Md,
    Json,
    /// A human-oriented call/dependency graph as a mermaid `flowchart`
    /// document — opt-in, aimed at GitHub's native mermaid rendering in PR
    /// comments/descriptions, not the default Markdown output.
    // See ADR 0021 for the design behind this output format.
    Mermaid,
}
impl From<Format> for OutputFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Md => OutputFormat::Markdown,
            Format::Json => OutputFormat::Json,
            Format::Mermaid => OutputFormat::Mermaid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_default_to_markdown_head_and_no_base_when_no_args_given() {
        let expected = Cli {
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
        let actual = Cli::parse_from(["rinkaku"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_tui_when_tui_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: true,
        };
        let actual = Cli::parse_from(["rinkaku", "--tui"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_tui_and_format_given_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--tui", "--format", "json"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_reject_format_and_tui_given_together_regardless_of_argument_order() {
        // clap's conflicts_with is declared on `format` (see Cli's own
        // `#[arg(...)]` attribute), but conflicts are symmetric regardless
        // of which flag declares the attribute or which one is passed
        // first on the command line — this pins that symmetry rather than
        // only ever exercising the --tui-first ordering above.
        let actual = Cli::try_parse_from(["rinkaku", "--format", "json", "--tui"]);

        assert!(actual.is_err());
    }
    #[test]
    fn should_set_base_when_base_flag_given() {
        let expected = Cli {
            command: None,
            base: Some("main".to_string()),
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_base_and_head_when_both_flags_given() {
        let expected = Cli {
            command: None,
            base: Some("main".to_string()),
            head: "feature-branch".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main", "--head", "feature-branch"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_format_json_when_format_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: Some(Format::Json),
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--format", "json"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_unknown_format_value() {
        let actual = Cli::try_parse_from(["rinkaku", "--format", "yaml"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_deps_zero_when_deps_flag_given() {
        let expected = Cli {
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
        let actual = Cli::parse_from(["rinkaku", "--deps", "0"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_deps_value_outside_zero_or_one() {
        let actual = Cli::try_parse_from(["rinkaku", "--deps", "2"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_exclude_tests_when_exclude_tests_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: true,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--exclude-tests"]);

        assert_eq!(expected, actual);
    }

    // ADR 0025's flipped default: with no test-related flag given, the
    // parsed `Cli` must land on `exclude_tests: false` — i.e. tests are
    // included in Change graph/Definitions by default. The
    // `should_default_to_markdown_head_and_no_base_when_no_args_given`
    // above already exercises the whole default `Cli` shape, but this
    // one pins the ADR 0025 decision specifically so a future default
    // flip has to update this test on purpose rather than by
    // consequence.
    #[test]
    fn should_default_to_including_tests_when_no_flag_given() {
        let actual = Cli::parse_from(["rinkaku"]);

        assert!(!actual.exclude_tests);
    }

    // Companion to the above: passing the old `--include-tests` flag
    // must now fail parsing, so a stale script surfaces as an error
    // instead of silently doing nothing. Pins the CLI break called out
    // in ADR 0025's Consequences.
    #[test]
    fn should_reject_the_removed_include_tests_flag() {
        let actual = Cli::try_parse_from(["rinkaku", "--include-tests"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_include_generated_when_include_generated_flag_given() {
        let expected = Cli {
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
        let actual = Cli::parse_from(["rinkaku", "--include-generated"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_entry_when_entry_flag_given() {
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: Some("src/api".to_string()),
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--entry", "src/api"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_self_update_command_when_self_update_subcommand_given() {
        let expected = Cli {
            command: Some(Command::SelfUpdate { yes: false }),
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
        let actual = Cli::parse_from(["rinkaku", "self-update"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_yes_flag_when_self_update_yes_flag_given() {
        let expected = Cli {
            command: Some(Command::SelfUpdate { yes: true }),
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
        let actual = Cli::parse_from(["rinkaku", "self-update", "--yes"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_yes_flag_when_self_update_short_y_flag_given() {
        let expected = Cli {
            command: Some(Command::SelfUpdate { yes: true }),
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
        let actual = Cli::parse_from(["rinkaku", "self-update", "-y"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_verify_cli_definition() {
        // clap's own consistency check (duplicate args, invalid
        // configuration, etc.) — mirrors skem's `Cli::command().debug_assert()`
        // convention for catching CLI wiring mistakes at test time.
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn should_set_pr_when_pr_flag_given() {
        // Also covers that `--pr` alone (no explicit `--head`) parses
        // successfully: `--head` has a default value, so clap's
        // `conflicts_with` must not fire unless `--head` was actually
        // passed on the command line — this is the behavior the ADR relies
        // on to let `--pr` reuse the `Cli` struct's `head` field internally
        // without users needing to omit an unrelated flag.
        let expected = Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: Some("76".to_string()),
            format: None,
            deps: 1,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--pr", "76"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_pr_and_base_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--pr", "76", "--base", "main"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_reject_pr_and_explicit_head_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--pr", "76", "--head", "feature-branch"]);

        assert!(actual.is_err());
    }
}
