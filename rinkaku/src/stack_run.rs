//! Stacked-PR driver for `--tui --pr` (ADR 0075): discovers the stack,
//! resolves every layer's SHAs up front, analyses the cursor layer behind
//! the splash screen, and hands the rest to one background thread that
//! fills the TUI's `PrAnalysisCache` while the reviewer reads.

use crate::cli::Cli;
use crate::github::base_sha::fetch_pr_heads;
use crate::github::pr_arg::{PrArg, parse_pr_arg};
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
    let cache = Arc::new(PrAnalysisCache::new(layers.len()));
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

fn resolve_layer_shas(
    plan: &StackPlan,
    progress: &dyn AnalysisProgress,
) -> anyhow::Result<Vec<ResolvedLayer>> {
    let numbers: Vec<u64> = plan.stack.prs.iter().map(|pr| pr.number).collect();
    let heads = fetch_pr_heads(&numbers, plan.workdir.as_deref())?;
    let _ = (progress, heads);
    todo!(
        "verify each head via ensure_fetched_head_matches, resolve each base via resolve_pr_base_sha"
    )
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
    for index in prefetch_order(plan.cursor, layers.len()) {
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

fn prefetch_order(cursor: usize, len: usize) -> Vec<usize> {
    todo!("order the {len} layers around cursor {cursor}")
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
        head_sha: layer.head_sha.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case::should_walk_up_then_wrap_to_bottom_when_cursor_is_mid_stack(1, 4, vec![2, 3, 0])]
    #[case::should_walk_up_only_when_cursor_is_at_the_bottom(0, 3, vec![1, 2])]
    #[case::should_walk_bottom_up_when_cursor_is_at_the_top(2, 3, vec![0, 1])]
    #[case::should_return_empty_when_stack_has_one_layer(0, 1, vec![])]
    #[ignore = "not implemented"]
    fn prefetch_order_cases(
        #[case] cursor: usize,
        #[case] len: usize,
        #[case] expected: Vec<usize>,
    ) {
        let actual = prefetch_order(cursor, len);
        assert_eq!(expected, actual);
    }
}
