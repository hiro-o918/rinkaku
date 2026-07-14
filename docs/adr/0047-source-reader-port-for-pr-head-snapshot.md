# 0047. Inject a `SourceReader` port so `--pr` mode reads the source view from the head snapshot

- Status: accepted
- Date: 2026-07-15
- Related: [ADR 0046](0046-source-view-diff-overlay.md) (its
  Consequences section flags this exact gap as a follow-up), [ADR
  0016](0016-tui-crate-and-stack.md) (`rinkaku-tui` may perform the
  source view's file read, but must not shell out to `git` itself)

## Context

ADR 0046's diff overlay disables itself (falls back to plain
rendering, with a pane-title note) whenever the source view's file
content doesn't match the diff's recorded context lines. For `--pr`
mode this fires on essentially every file the PR actually changes:
`--pr` fetches the PR's head ref (`refs/pull/N/head`) into the
resolved workdir but never checks it out (`main.rs`'s own module doc
comment on its `--pr` read strategy), so
`rinkaku_tui::source::load_symbol_source` keeps reading whatever the
working tree happened to hold before `rinkaku` ran — unrelated to the
PR's new side. The overlay's drift check correctly detects this and
disables itself, but the result is that `--pr` mode — the input mode a
reviewer reaches for most often — is exactly where the diff overlay is
least available.

`rinkaku-tui` must not run `git` itself (ADR 0016 decision 3: only
`main.rs`/adapters own IO beyond the one source-file read the source
view already makes). Fixing the gap therefore means `main.rs` handing
`rinkaku-tui` a different way to read a file's content for `--pr`
mode specifically — `git show <head SHA>:<path>`, the same read
strategy `rinkaku/src/git/file_read.rs::read_git_show_file` already
uses for `--base`/`--pr`'s own diff-analysis pipeline (`pipeline.rs`).

## Decision

**1. `rinkaku-tui/src/source.rs` gains a `SourceReader` trait**, one
method: `fn read(&self, repo_root: &Path, relative_path: &str) ->
Result<String, String>`. Defined in `source.rs` (the consumer), not in
`rinkaku`'s adapter layer — same "port defined where it's used" rule
this project's architecture conventions already apply everywhere
else. `load_symbol_source`/`load_highlighted_symbol_source` take
`reader: &dyn SourceReader` and call `reader.read(...)` instead of
`std::fs::read_to_string` directly.

**2. `WorkingTreeSourceReader` (in the same module) is the default
implementation**, wrapping exactly the `resolve_source_path` +
`std::fs::read_to_string` logic the source view always used before
this ADR. Every input mode except `--pr` (stdin, `--base`, and `--pr`
before this ADR) keeps this reader — no behavior change for them.

**3. `rinkaku`'s adapter layer gains `PrHeadSourceReader`**
(`rinkaku/src/git/file_read.rs`), implementing `SourceReader` by
calling the existing `read_git_show_file(cwd, head, path)` — the same
function `pipeline.rs` already uses for `--base`/`--pr`'s own file
reads, so this is a new caller of existing IO, not new IO.

**4. `main.rs` (composition root) wires the reader per input mode.**
`run_analysis`'s `--pr` branch already resolves both the values this
reader needs — the workdir (`resolve_pr_workdir`) and the head SHA
(`fetch_pr_head`) — so `AnalyzedReport` grows a `pr_head_sha: Option<String>`
field (`None` for every mode but `--pr`, mirroring how
`resolved_workdir` already carries the workdir out). The
`DisplayMode::Tui` branch constructs `PrHeadSourceReader { head,
cwd: resolved_workdir }` when `pr_head_sha` is `Some`, else uses
`WorkingTreeSourceReader`, and passes `&dyn SourceReader` into
`TuiSession::run` (itself threading it through to `run_app`).

## Alternatives

- **Plumb the head SHA into `rinkaku-tui` and let it call `git show`
  itself.** Rejected: ADR 0016 draws the line at "one source-file
  read" precisely to keep `rinkaku-tui` free of process-spawning IO;
  adding a `git show` invocation inside the TUI crate would cross that
  line for a capability `rinkaku`'s adapter layer already has.
- **Branch inside `load_symbol_source` on an `Option<PrContext>`
  parameter instead of a trait.** Rejected as a worse shape for the
  same job: a trait keeps `source.rs` ignorant of what `--pr` even is
  (it only knows "a way to read a file"), whereas a context struct
  would leak `--pr`-specific concepts into a module every input mode
  shares, and every future read strategy (a hypothetical LSP-backed
  read, a remote content API) would need another branch instead of
  another impl.
- **Do nothing, keep the ADR 0046 Consequences note as accepted
  scope.** Rejected because the gap lands on the input mode used most
  during real PR review (see this project's own dogfooding workflow,
  which runs `--pr` for the map-assisted review pass) — the very
  scenario the diff overlay was built for.

## Consequences

- `--pr` mode's source view (`s`) now shows the diff overlay for
  files the PR changes, matching `--base` mode's behavior — no more
  "overlay unavailable" note for the common case.
- `--base` and stdin modes are unaffected: `WorkingTreeSourceReader`
  preserves their exact prior behavior (including the *existing*,
  documented drift risk when the working tree has been edited since
  the diff was produced or diffed against a different revision).
- `rinkaku-tui` gains one new public trait plus one new public struct
  in `source.rs`; no new file, no new module — the module stays under
  the file-size discipline's watch threshold.
- `rinkaku`'s adapter layer gains one new small struct
  (`PrHeadSourceReader`) reusing existing `read_git_show_file`
  plumbing; no new subprocess call shape.
- A future read strategy (e.g. an LSP-backed reader) has a clear
  extension point: implement `SourceReader`, wire it in at
  `main.rs`'s composition root — no changes needed inside `source.rs`
  itself.
