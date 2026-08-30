//! Stacked-PR session state (ADR 0075): the ordered layer list with a
//! cursor ([`StackPosition`]) and the shared cache the `rinkaku` binary's
//! background analysis thread fills while the reviewer reads the current
//! layer ([`PrAnalysisCache`]). Data and synchronisation only — no git/gh
//! knowledge lives in this crate.

use crate::review::PrContext;
use rinkaku_core::render::Report;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackEntry {
    pub number: u64,
    pub title: String,
    pub head_ref_name: String,
}

/// The stack's open layers bottom-first, plus which one is on screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackPosition {
    trunk: String,
    entries: Vec<StackEntry>,
    cursor: usize,
}

impl StackPosition {
    pub fn new(trunk: String, entries: Vec<StackEntry>, cursor: usize) -> Self {
        let clamped = cursor.min(entries.len().saturating_sub(1));
        Self {
            trunk,
            entries,
            cursor: clamped,
        }
    }

    pub fn trunk(&self) -> &str {
        &self.trunk
    }

    /// Up is away from trunk, matching `gh stack up`.
    pub fn move_up(&mut self) -> bool {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    pub fn move_down(&mut self) -> bool {
        if self.cursor > 0 {
            self.cursor -= 1;
            true
        } else {
            false
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[StackEntry] {
        &self.entries
    }

    pub fn current(&self) -> &StackEntry {
        &self.entries[self.cursor]
    }

    pub fn label(&self) -> String {
        format!(
            "PR #{} {}/{}",
            self.current().number,
            self.cursor + 1,
            self.entries.len()
        )
    }
}

/// One layer's analysis: everything `run_app` needs for a session over it.
#[derive(Debug)]
pub struct PrAnalysis {
    pub report: Report,
    pub diff_text: String,
    pub pr: PrContext,
}

#[derive(Debug, Clone)]
pub enum Slot {
    Pending,
    InProgress,
    Ready(Arc<PrAnalysis>),
    Failed(String),
}

/// Per-layer analysis slots shared between the binary's background thread
/// (writer) and the TUI driver (reader). The driver publishes the cursor
/// so the worker always picks the layer the reviewer is most likely to
/// open next (ADR 0075 D3).
#[derive(Debug)]
pub struct PrAnalysisCache {
    slots: Mutex<Vec<Slot>>,
    ready: Condvar,
    cursor: AtomicUsize,
}

/// The pending layer nearest to `cursor` (the cursor's own layer first),
/// preferring the one above on a tie so a bottom-up reviewer's next layer
/// is ready first.
pub fn next_prefetch_target(cursor: usize, pending: &[bool]) -> Option<usize> {
    let here = (cursor < pending.len()).then_some((0, cursor));
    let above = (cursor + 1..pending.len()).map(|i| (i - cursor, i));
    let below = (0..cursor).rev().map(|i| (cursor - i, i));
    here.into_iter()
        .chain(above)
        .chain(below)
        .filter(|&(_, index)| pending[index])
        .min_by_key(|&(distance, index)| (distance, index <= cursor))
        .map(|(_, index)| index)
}

impl PrAnalysisCache {
    pub fn new(len: usize, cursor: usize) -> Self {
        Self {
            slots: Mutex::new(vec![Slot::Pending; len]),
            ready: Condvar::new(),
            cursor: AtomicUsize::new(cursor),
        }
    }

    pub fn set_cursor(&self, cursor: usize) {
        self.cursor.store(cursor, Ordering::Relaxed);
    }

    /// Marks the next layer to analyse as `InProgress` and returns its
    /// index, or `None` once every layer is taken.
    pub fn claim_next(&self) -> Option<usize> {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let pending: Vec<bool> = slots
            .iter()
            .map(|slot| matches!(slot, Slot::Pending))
            .collect();
        let index = next_prefetch_target(self.cursor.load(Ordering::Relaxed), &pending)?;
        slots[index] = Slot::InProgress;
        Some(index)
    }

    pub fn set(&self, index: usize, slot: Slot) {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slots[index] = slot;
        self.ready.notify_all();
    }

    pub fn get(&self, index: usize) -> Slot {
        let slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        slots[index].clone()
    }

    pub fn wait_ready(&self, index: usize) -> Result<Arc<PrAnalysis>, String> {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            match &slots[index] {
                Slot::Ready(analysis) => return Ok(Arc::clone(analysis)),
                Slot::Failed(message) => return Err(message.clone()),
                Slot::Pending | Slot::InProgress => {
                    slots = self
                        .ready
                        .wait(slots)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    fn entries() -> Vec<StackEntry> {
        vec![
            StackEntry {
                number: 42,
                title: "auth".to_string(),
                head_ref_name: "auth".to_string(),
            },
            StackEntry {
                number: 43,
                title: "api".to_string(),
                head_ref_name: "api".to_string(),
            },
            StackEntry {
                number: 44,
                title: "frontend".to_string(),
                head_ref_name: "frontend".to_string(),
            },
        ]
    }

    #[test]
    fn should_advance_cursor_when_moving_up_below_the_top() {
        let mut position = StackPosition::new("main".to_string(), entries(), 0);

        let moved = position.move_up();

        assert_eq!((true, 1), (moved, position.cursor()));
    }

    #[test]
    fn should_clamp_cursor_when_moving_up_at_the_top() {
        let mut position = StackPosition::new("main".to_string(), entries(), 2);

        let moved = position.move_up();

        assert_eq!((false, 2), (moved, position.cursor()));
    }

    #[test]
    fn should_clamp_cursor_when_moving_down_at_the_bottom() {
        let mut position = StackPosition::new("main".to_string(), entries(), 0);

        let moved = position.move_down();

        assert_eq!((false, 0), (moved, position.cursor()));
    }

    #[test]
    fn should_render_pr_number_and_one_based_position_when_labelled() {
        let position = StackPosition::new("main".to_string(), entries(), 1);

        let actual = position.label();

        assert_eq!("PR #43 2/3".to_string(), actual);
    }

    #[test]
    fn should_expose_the_trunk_name_passed_to_new() {
        let position = StackPosition::new("main".to_string(), entries(), 0);

        let actual = position.trunk();

        assert_eq!("main", actual);
    }

    #[rstest]
    #[case::should_pick_the_cursor_layer_when_it_is_still_pending(2, &[true, true, true, true, true], Some(2))]
    #[case::should_pick_the_layer_above_when_both_neighbours_are_pending(2, &[true, true, false, true, true], Some(3))]
    #[case::should_pick_the_layer_below_when_the_one_above_is_taken(2, &[true, true, false, false, true], Some(1))]
    #[case::should_expand_outward_when_neighbours_are_taken(2, &[true, false, false, false, true], Some(4))]
    #[case::should_walk_down_when_cursor_is_at_the_top(2, &[true, true, false], Some(1))]
    #[case::should_return_none_when_nothing_is_pending(0, &[false, false], None)]
    #[case::should_return_none_when_stack_has_one_layer(0, &[false], None)]
    fn next_prefetch_target_cases(
        #[case] cursor: usize,
        #[case] pending: &[bool],
        #[case] expected: Option<usize>,
    ) {
        let actual = next_prefetch_target(cursor, pending);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_claim_layers_nearest_to_the_published_cursor_first() {
        let cache = PrAnalysisCache::new(4, 0);

        let first = cache.claim_next();
        let second = cache.claim_next();
        cache.set_cursor(3);
        let third = cache.claim_next();
        let fourth = cache.claim_next();

        assert_eq!(
            vec![Some(0), Some(1), Some(3), Some(2)],
            vec![first, second, third, fourth]
        );
    }

    #[test]
    fn should_stop_claiming_when_every_layer_is_taken() {
        let cache = PrAnalysisCache::new(2, 0);
        cache.claim_next();
        cache.claim_next();

        let actual = cache.claim_next();

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_pending_when_slot_has_not_been_set() {
        let cache = PrAnalysisCache::new(2, 0);

        let actual = cache.get(1);

        assert!(matches!(actual, Slot::Pending));
    }

    #[test]
    fn should_return_failure_message_when_slot_failed() {
        let cache = PrAnalysisCache::new(1, 0);
        cache.set(0, Slot::Failed("boom".to_string()));

        let actual = cache.wait_ready(0);

        assert_eq!(Err("boom".to_string()), actual.map(|_| ()));
    }

    #[test]
    fn should_unblock_waiter_when_slot_is_set_from_another_thread() {
        let cache = Arc::new(PrAnalysisCache::new(1, 0));
        let writer = Arc::clone(&cache);
        std::thread::spawn(move || writer.set(0, Slot::Failed("late".to_string())));

        let actual = cache.wait_ready(0);

        assert_eq!(Err("late".to_string()), actual.map(|_| ()));
    }
}
