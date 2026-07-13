//! GitHub-specific glue: PR argument parsing, `gh pr view` info fetch,
//! GitHub remote URL parsing, PR workdir resolution (cwd / ghq / cache
//! clone), and PR base-SHA resolution (ADR 0004, 0005, 0006, 0007).

pub(crate) mod base_sha;
pub(crate) mod pr_arg;
pub(crate) mod pr_info;
pub(crate) mod remote;
pub(crate) mod workdir;
