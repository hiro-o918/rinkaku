# 0031. Parallel whole-repo tree-sitter parse with rayon

- Status: accepted
- Date: 2026-07-13

## Context

ADR 0017 made bare `rinkaku` build a whole-repository outline as the
default when stdin is a terminal, so every tracked file in the working
tree is parsed on TUI startup. Profiling that path on real repositories
(release build, `time script -q /dev/null rinkaku --format json`, mean of
three runs) shows the pipeline is dominated by tree-sitter parsing:

| repository       | tracked files | wall time |
|------------------|--------------:|----------:|
| rinkaku (self)   |           122 |    327 ms |
| cli/cli          |         ~2100 |   1600 ms |
| astral-sh/ruff   |          9327 |  10440 ms |

An in-process sampling profile attributed 94–98 % of the time to the
per-file `extract_all_symbols` invocation, which parses the file with
`tree_sitter::Parser` and runs the definition/reference queries against
the resulting tree. The work is embarrassingly parallel — every file is
parsed independently, no shared state, no ordering constraint between
files — and today `analyze_repo`'s per-file loop is a plain sequential
`for path in paths` that reads, parses, and pushes into `Vec<FileReport>`
one file at a time on a single core.

Two observations shape the choice of parallelism primitive:

- `extract.rs::with_definition_nodes` already constructs a fresh
  `tree_sitter::Parser` and compiles the definition/reference queries
  per call. There is no long-lived parser instance to share, so the only
  cross-thread hazard is whether the per-call parser can safely live on
  a worker thread rather than the caller's. `tree_sitter::Parser` is
  `Send` (not `Sync`), which matches rayon's per-task ownership model.
- The pipeline's `read_file` port is currently `impl Fn(&str) ->
  io::Result<String>`. Parallelising the loop requires `Sync + Send` on
  the port so worker threads can call it concurrently, but every
  real-world implementer (`main.rs::read_working_tree_file`, tests'
  in-memory `HashMap`-backed closures) already satisfies both auto
  traits.

## Decision

Introduce `rayon` as a `rinkaku-core` dependency and parallelise the
per-file loop in `pipeline::analyze_repo` with `par_iter`.
`extract_all_symbols` is called once per file inside the parallel map
stage; downstream work (`build_graph`, `stamp_ids`, `compute_hotspots`,
`compute_file_size_warnings`) stays sequential because it operates on
the already-collected `Vec<FileReport>`.

Determinism of the output is preserved by relying on rayon's contract
that `par_iter().collect::<Vec<_>>()` yields elements in source-order.
The per-file filter (`Option`-returning map + `flatten`) and the final
`Vec<FileReport>` therefore appear in the same order as the input
`paths`, which is what today's sequential loop already produces. The
regression test `should_produce_deterministic_output_on_repeated_calls`
pins this so any future accidental switch to an ordering-breaking
combinator (e.g. `par_bridge` or unordered `flat_map`) will fail loudly.

The pipeline's `read_file` port signature is strengthened from `impl Fn`
to `impl Fn + Sync + Send`. This is a bound addition; every existing
caller — `main.rs::read_working_tree_file` (a bare `fn`) and the tests'
`fake_reader` closures — already satisfies the new bound without any
change at the call site.

`analyze_diff`'s own per-file loop is not parallelised. The diff mode's
file count is bounded by "files touched by a single PR" — small enough
in practice (rarely more than a few dozen) that the fixed overhead of
spawning a rayon job pool outweighs the win, and the loop's per-file
body carries diff-specific side data (`skipped`, `removed`,
`sized_files`, `read_base_file` interactions) that would need
reshuffling into a `collect`-friendly shape. If diff-mode file counts
grow (e.g. a future whole-directory `--base HEAD~1000` invocation), that
loop can be revisited with the same pattern.

## Alternatives

- **Progressive TUI rendering** (start the outline empty, populate rows
  as parses complete): improves *perceived* latency but is a larger
  architectural change to `rinkaku-tui`'s state machine and orthogonal
  to the underlying parse cost. Deferred to a separate ADR/PR.
- **Lazy start** (open the TUI on an empty `Report`, kick off the parse
  in a background thread, hand results to the UI incrementally):
  strictly larger than the above — needs a cross-thread channel between
  core and TUI, and complicates ordering guarantees the renderers today
  take for granted. Deferred.
- **Manual `std::thread` dispatch** with a hand-rolled work queue:
  rayon's `par_iter` gives work-stealing, per-CPU sizing, and panic
  propagation out of the box; hand-rolling those is pure downside.
- **`tokio::task::spawn_blocking` per file**: adds a `tokio` runtime to
  `rinkaku-core`, which currently has no async surface at all. Not
  justified for a CPU-bound workload where rayon is a better fit.

## Consequences

- **Performance**: on release builds, `analyze_repo` scales with
  available cores. Measured on an M-series 10-core laptop after the
  change: rinkaku (self) 327 → ~120 ms, cli/cli 1600 → ~360 ms, ruff
  10440 → ~1700 ms (see PR body for the exact runs). The ratio widens
  with file count, matching the "parse is 94–98 % of wall time" profile.
- **Dependencies**: `rinkaku-core` gains `rayon` (workspace-level entry
  in the root `Cargo.toml`, followed by the same crates that re-export
  workspace deps in their own `[dependencies]` sections). `Cargo.lock`
  is updated.
- **API stability**: `analyze_repo`'s public signature gains
  `+ Sync + Send` on its `read_file` port. `main.rs` and every existing
  test compile unchanged because their `read_file` implementations
  already satisfy the new bounds. `analyze_diff`'s signature is not
  changed.
- **Pure-core invariant (ADR 0016 shorthand)**: "pure" here means "no IO
  or side effects visible from outside", not "single-threaded". Rayon
  moves work between threads but does not introduce new observable side
  effects — the function still consumes plain data and returns plain
  data. Determinism is preserved (source order), so the output for a
  given input is unchanged.
- **Testing**: the existing 1033-test workspace pass on the
  parallelised implementation without modification (rayon's `par_iter`
  ordering contract makes the outputs bit-identical). One regression
  test is added to lock the determinism guarantee in place.
- **Debuggability**: panics inside the per-file body now surface via
  rayon's default panic-propagation (the first panicking thread's
  payload is re-raised at the `collect` site). No behavioural change
  for the non-panicking path, which is what the pipeline design targets
  today — `extract_all_symbols`'s expects are already load-bearing
  invariants, not user-triggerable failures.
