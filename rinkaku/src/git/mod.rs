//! Local git subprocess wrappers: `git diff`, `git ls-files`,
//! `git rev-parse --show-toplevel`, working-tree/`git show` file reads,
//! and the `git cat-file --batch` bulk reader used by
//! `pipeline::build_resolver`.

pub(crate) mod cat_file_batch;
pub(crate) mod commands;
pub(crate) mod file_read;
