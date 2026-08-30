//! Stacked-PR session state (ADR 0075): the ordered layer list with a
//! cursor ([`StackPosition`]) and the shared cache the `rinkaku` binary's
//! background analysis thread fills while the reviewer reads the current
//! layer ([`PrAnalysisCache`]). Data and synchronisation only — no git/gh
//! knowledge lives in this crate.

use crate::review::PrContext;
use rinkaku_core::render::Report;
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
    entries: Vec<StackEntry>,
    cursor: usize,
}

impl StackPosition {
    pub fn new(entries: Vec<StackEntry>, cursor: usize) -> Self {
        todo!(
            "build a StackPosition over {} entries at {cursor}",
            entries.len()
        )
    }

    /// Up is away from trunk, matching `gh stack up`.
    pub fn move_up(&mut self) -> bool {
        todo!("advance the cursor toward the top of the stack")
    }

    pub fn move_down(&mut self) -> bool {
        todo!("retreat the cursor toward the bottom of the stack")
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
        todo!("return the entry under the cursor")
    }

    pub fn label(&self) -> String {
        todo!("format the cursor's PR number and 1-based position")
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
    Ready(Arc<PrAnalysis>),
    Failed(String),
}

/// Per-layer analysis slots shared between the binary's background thread
/// (writer) and the TUI driver (reader).
#[derive(Debug)]
pub struct PrAnalysisCache {
    slots: Mutex<Vec<Slot>>,
    ready: Condvar,
}

impl PrAnalysisCache {
    pub fn new(len: usize) -> Self {
        Self {
            slots: Mutex::new(vec![Slot::Pending; len]),
            ready: Condvar::new(),
        }
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
                Slot::Pending => {
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
    #[ignore = "not implemented"]
    fn should_advance_cursor_when_moving_up_below_the_top() {
        let mut position = StackPosition::new(entries(), 0);

        let moved = position.move_up();

        assert_eq!((true, 1), (moved, position.cursor()));
    }

    #[test]
    #[ignore = "not implemented"]
    fn should_clamp_cursor_when_moving_up_at_the_top() {
        let mut position = StackPosition::new(entries(), 2);

        let moved = position.move_up();

        assert_eq!((false, 2), (moved, position.cursor()));
    }

    #[test]
    #[ignore = "not implemented"]
    fn should_clamp_cursor_when_moving_down_at_the_bottom() {
        let mut position = StackPosition::new(entries(), 0);

        let moved = position.move_down();

        assert_eq!((false, 0), (moved, position.cursor()));
    }

    #[test]
    #[ignore = "not implemented"]
    fn should_render_pr_number_and_one_based_position_when_labelled() {
        let position = StackPosition::new(entries(), 1);

        let actual = position.label();

        assert_eq!("PR #43 2/3".to_string(), actual);
    }

    #[test]
    fn should_return_pending_when_slot_has_not_been_set() {
        let cache = PrAnalysisCache::new(2);

        let actual = cache.get(1);

        assert!(matches!(actual, Slot::Pending));
    }

    #[test]
    fn should_return_failure_message_when_slot_failed() {
        let cache = PrAnalysisCache::new(1);
        cache.set(0, Slot::Failed("boom".to_string()));

        let actual = cache.wait_ready(0);

        assert_eq!(Err("boom".to_string()), actual.map(|_| ()));
    }

    #[test]
    fn should_unblock_waiter_when_slot_is_set_from_another_thread() {
        let cache = Arc::new(PrAnalysisCache::new(1));
        let writer = Arc::clone(&cache);
        std::thread::spawn(move || writer.set(0, Slot::Failed("late".to_string())));

        let actual = cache.wait_ready(0);

        assert_eq!(Err("late".to_string()), actual.map(|_| ()));
    }
}
