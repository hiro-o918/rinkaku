//! Composition root for the `rinkaku` binary.
//!
//! This is the only place allowed to know about the concrete CLI wiring.
//! It stays a thin entry point: parse arguments, initialize logging, and
//! (once implemented) dispatch to the pure core in `lib.rs` via ports such
//! as `LanguageSupport` and `Resolver`.

use clap::Parser;

/// rinkaku (輪郭) — condense PR diffs into signatures and their dependencies.
#[derive(Parser, Debug)]
#[command(name = "rinkaku", version, about, long_about = None)]
struct Cli {
    /// Base ref to diff against (runs `git diff` internally instead of
    /// reading from stdin). Not yet implemented.
    #[arg(long)]
    base: Option<String>,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let _cli = Cli::parse();

    anyhow::bail!("rinkaku is not implemented yet");
}
