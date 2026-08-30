//! Stacked-PR driver for `--tui --pr` (ADR 0075): discovers the stack,
//! resolves every layer's SHAs up front, analyses the cursor layer behind
//! the splash screen, and hands the rest to one background thread that
//! fills the TUI's `PrAnalysisCache` while the reviewer reads.

use crate::cli::Cli;
use crate::github::base_sha::{
    fetch_branch_head, fetch_oid, fetch_pr_heads, object_exists_locally, resolve_pr_base_sha,
};
use crate::github::pr_arg::{PrArg, parse_pr_arg};
use crate::github::pr_info::ensure_fetched_head_matches;
use crate::github::remote::{git_remote_origin_url, parse_github_remote};
use crate::github::stack::{PrStack, StackPr, fetch_pr_stack};
use crate::github::workdir::resolve_pr_workdir;
use crate::pipeline::run_base_pipeline;
use crate::progress::AnalysisProgress;
use crate::spinner::AnalysisPhase;
use crate::splash_progress::SplashProgress;
use rinkaku_tui::locale::Locale;
use rinkaku_tui::review::PrContext;
use rinkaku_tui::stack::{PrAnalysis, PrAnalysisCache, Slot, StackEntry, StackPosition};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct StackPlan {
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) stack: PrStack,
    pub(crate) cursor: usize,
    pub(crate) workdir: Option<PathBuf>,
}

struct ResolvedLayer {
    pr: StackPr,
    base_sha: String,
    head_sha: String,
}

/// `Ok(None)` unless `--pr` names an open layer of a GitHub stack and
/// `--no-stack` is absent.
pub(crate) fn discover_stack(
    cli: &Cli,
    progress: &dyn AnalysisProgress,
) -> anyhow::Result<Option<StackPlan>> {
    let Some(pr_arg) = &cli.pr else {
        return Ok(None);
    };
    if cli.no_stack {
        return Ok(None);
    }
    progress.set_phase(AnalysisPhase::ResolvingPr);
    let parsed = parse_pr_arg(pr_arg)?;
    let workdir = resolve_pr_workdir(&parsed)?;
    let (owner, repo) = match &parsed {
        PrArg::Url { owner, repo, .. } => (owner.clone(), repo.clone()),
        PrArg::Number(_) => {
            let Some(origin) = git_remote_origin_url(workdir.as_deref())? else {
                return Ok(None);
            };
            let Some(parsed_remote) = parse_github_remote(&origin) else {
                return Ok(None);
            };
            parsed_remote
        }
    };
    let Some(stack) = fetch_pr_stack(&owner, &repo, parsed.number())? else {
        return Ok(None);
    };
    let Some(cursor) = stack.position_of(parsed.number()) else {
        return Ok(None);
    };
    Ok(Some(StackPlan {
        owner,
        repo,
        stack,
        cursor,
        workdir,
    }))
}

/// The returned flag is [`TuiSession::run`]'s: whether a self-update was
/// requested from inside the TUI.
///
/// [`TuiSession::run`]: rinkaku_tui::TuiSession::run
pub(crate) fn run_stack_session(
    cli: &Cli,
    plan: StackPlan,
    progress: SplashProgress,
    update_check: Option<std::sync::mpsc::Receiver<String>>,
    locale: Locale,
) -> anyhow::Result<bool> {
    let layers = resolve_layer_shas(&plan, &progress)?;
    let cache = Arc::new(PrAnalysisCache::new(layers.len(), plan.cursor));
    let initial = analyze_layer(cli, &plan, &layers[plan.cursor], &progress)?;
    cache.set(plan.cursor, Slot::Ready(Arc::new(initial)));
    let (session, buffered_notes) = progress.into_session_and_notes();

    let position = StackPosition::new(stack_entries(&layers), plan.cursor);
    let stack_pr_contexts: Vec<PrContext> = layers
        .iter()
        .map(|layer| pr_context(&plan, layer))
        .collect();
    let cancel = AtomicBool::new(false);
    let run_result = std::thread::scope(|scope| {
        scope.spawn(|| analyze_remaining_layers(cli, &plan, &layers, &cache, &cancel));
        let source_reader_for = |ctx: &PrContext| -> Box<dyn rinkaku_tui::source::SourceReader> {
            Box::new(crate::git::file_read::PrHeadSourceReader {
                head: ctx.head_sha.clone(),
                cwd: plan.workdir.clone(),
            })
        };
        let system_clipboard = crate::clipboard::SystemClipboard::detect();
        let review_ports = rinkaku_tui::ReviewPorts {
            pr_context: None,
            stack_pr_contexts,
            submitter: Some(&crate::github::review::GhReviewSubmitter),
            clipboard: &system_clipboard,
            browser: &crate::browser::SystemBrowserOpener,
        };
        let result = session.run_stack(
            position,
            Arc::clone(&cache),
            &source_reader_for,
            cli.entry.as_deref(),
            &crate::git::commands::resolve_repo_root(plan.workdir.as_deref()),
            review_ports,
            update_check,
            locale,
        );
        cancel.store(true, Ordering::Relaxed);
        result
    });
    crate::flush_notes(buffered_notes);
    Ok(run_result?)
}

/// The outcome of resolving one stack layer's base commit: the resolved SHA
/// and, when the availability cascade (ADR 0007) fell all the way back to
/// the base branch's tip, the note to surface through [`AnalysisProgress`]
/// the same way the single-PR path does.
struct LayerBaseResolution {
    base_sha: String,
    fallback_note: Option<String>,
}

/// Verifies `pr`'s fetched head against what GitHub reported, then resolves
/// its base commit through the same [`resolve_pr_base_sha`] cascade the
/// single-PR path uses (`main.rs`'s `run_analysis`) — pure aside from the
/// injected closures, so it is unit-testable without shelling out to `git`.
fn resolve_layer(
    pr: &StackPr,
    fetched_head: &str,
    object_exists: impl FnMut(&str) -> bool,
    fetch_base_branch: impl FnMut() -> anyhow::Result<String>,
    fetch_oid: impl FnMut(&str) -> anyhow::Result<()>,
) -> anyhow::Result<LayerBaseResolution> {
    ensure_fetched_head_matches(pr.number, fetched_head, &pr.head_ref_oid)?;
    let (base_sha, used_fallback) = resolve_pr_base_sha(
        &pr.base_ref_oid,
        object_exists,
        fetch_base_branch,
        fetch_oid,
    )?;
    let fallback_note = used_fallback.then(|| {
        format!(
            "warning: could not resolve PR #{number}'s base commit ({base_oid}) locally; \
             falling back to the current tip of {base_branch}, which may not reproduce the \
             original PR diff for a merged PR",
            number = pr.number,
            base_oid = pr.base_ref_oid,
            base_branch = pr.base_ref_name,
        )
    });
    Ok(LayerBaseResolution {
        base_sha,
        fallback_note,
    })
}

fn resolve_layer_shas(
    plan: &StackPlan,
    progress: &dyn AnalysisProgress,
) -> anyhow::Result<Vec<ResolvedLayer>> {
    let numbers: Vec<u64> = plan.stack.prs.iter().map(|pr| pr.number).collect();
    let cwd = plan.workdir.as_deref();
    let heads = fetch_pr_heads(&numbers, cwd)?;

    plan.stack
        .prs
        .iter()
        .zip(heads)
        .map(|(pr, head_sha)| {
            let resolution = resolve_layer(
                pr,
                &head_sha,
                |oid| object_exists_locally(cwd, oid),
                || fetch_branch_head(&pr.base_ref_name, cwd),
                |oid| fetch_oid(cwd, oid),
            )?;
            if let Some(note) = resolution.fallback_note {
                progress.note(note);
            }
            Ok(ResolvedLayer {
                pr: pr.clone(),
                base_sha: resolution.base_sha,
                head_sha,
            })
        })
        .collect()
}

fn analyze_layer(
    cli: &Cli,
    plan: &StackPlan,
    layer: &ResolvedLayer,
    progress: &dyn AnalysisProgress,
) -> anyhow::Result<PrAnalysis> {
    let (report, diff_text) = run_base_pipeline(
        cli,
        &layer.base_sha,
        &layer.head_sha,
        plan.workdir.as_deref(),
        progress,
    )?;
    Ok(PrAnalysis {
        report,
        diff_text,
        pr: pr_context(plan, layer),
    })
}

/// `cancel` is only checked between layers: a running analysis is left to
/// finish so the scoped thread joins within one layer's analysis time.
fn analyze_remaining_layers(
    cli: &Cli,
    plan: &StackPlan,
    layers: &[ResolvedLayer],
    cache: &PrAnalysisCache,
    cancel: &AtomicBool,
) {
    while let Some(index) = cache.claim_next() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let slot = match analyze_layer(cli, plan, &layers[index], &SilentProgress) {
            Ok(analysis) => Slot::Ready(Arc::new(analysis)),
            Err(err) => Slot::Failed(err.to_string()),
        };
        cache.set(index, slot);
    }
}

/// The background thread's progress sink: the splash screen belongs to the
/// main thread (ADR 0033 decision 2), so nothing is drawn or printed here.
struct SilentProgress;

impl AnalysisProgress for SilentProgress {
    fn set_phase(&self, _phase: AnalysisPhase) {}
    fn note(&self, _message: String) {}
}

fn stack_entries(layers: &[ResolvedLayer]) -> Vec<StackEntry> {
    layers
        .iter()
        .map(|layer| StackEntry {
            number: layer.pr.number,
            title: layer.pr.title.clone(),
            head_ref_name: layer.pr.head_ref_name.clone(),
        })
        .collect()
}

fn pr_context(plan: &StackPlan, layer: &ResolvedLayer) -> PrContext {
    PrContext {
        owner: plan.owner.clone(),
        repo: plan.repo.clone(),
        number: layer.pr.number,
        title: layer.pr.title.clone(),
        head_sha: layer.head_sha.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn stack_pr() -> StackPr {
        StackPr {
            number: 43,
            title: "api".to_string(),
            head_ref_name: "api".to_string(),
            base_ref_name: "auth".to_string(),
            base_ref_oid: "base789".to_string(),
            head_ref_oid: "head123".to_string(),
        }
    }

    #[test]
    fn should_resolve_base_without_fallback_when_head_matches_and_base_exists_locally() {
        let pr = stack_pr();

        let actual = resolve_layer(
            &pr,
            "head123",
            |_oid| true,
            || panic!("fetch_base_branch must not run when the base already exists locally"),
            |_oid| panic!("fetch_oid must not run when the base already exists locally"),
        )
        .expect("should resolve without error");

        assert_eq!("base789".to_string(), actual.base_sha);
        assert_eq!(None, actual.fallback_note);
    }

    #[test]
    fn should_error_when_fetched_head_does_not_match_the_reported_head() {
        let pr = stack_pr();

        let actual = resolve_layer(
            &pr,
            "unexpected-head",
            |_oid| true,
            || panic!("fetch_base_branch must not run when the head check fails first"),
            |_oid| panic!("fetch_oid must not run when the head check fails first"),
        );

        assert!(actual.is_err());
    }

    #[test]
    fn should_carry_a_fallback_note_when_the_base_cascade_falls_back_to_the_branch_tip() {
        let pr = stack_pr();

        let actual = resolve_layer(
            &pr,
            "head123",
            |_oid| false,
            || Ok("branch-tip-sha".to_string()),
            |_oid| anyhow::bail!("simulated: base789 not found on the remote"),
        )
        .expect("should fall back rather than error");

        assert_eq!("branch-tip-sha".to_string(), actual.base_sha);
        assert_eq!(
            Some(
                "warning: could not resolve PR #43's base commit (base789) locally; falling \
                 back to the current tip of auth, which may not reproduce the original PR diff \
                 for a merged PR"
                    .to_string()
            ),
            actual.fallback_note
        );
    }
}
