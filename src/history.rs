use std::collections::VecDeque;

/// Undo/redo over whole-value snapshots.
///
/// Callers `commit` the current value at natural checkpoints; consecutive
/// commits of an unchanged value are ignored, so a burst of edits between two
/// checkpoints becomes a single undo step.
#[derive(Debug, Clone)]
pub struct History<T> {
    checkpoint: T,
    undo: VecDeque<T>,
    redo: Vec<T>,
    limit: usize,
}

impl<T: Clone + PartialEq> History<T> {
    pub fn new(initial: T, limit: usize) -> Self {
        Self {
            checkpoint: initial,
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// Returns `true` when `current` differed from the last checkpoint.
    pub fn commit(&mut self, current: &T) -> bool {
        if *current == self.checkpoint {
            return false;
        }
        let previous = std::mem::replace(&mut self.checkpoint, current.clone());
        self.undo.push_back(previous);
        if self.undo.len() > self.limit {
            self.undo.pop_front();
        }
        self.redo.clear();
        true
    }

    /// Commits `current` first so uncommitted edits are not lost, then
    /// returns the value to restore.
    pub fn undo(&mut self, current: &T) -> Option<T> {
        self.commit(current);
        let previous = self.undo.pop_back()?;
        let undone = std::mem::replace(&mut self.checkpoint, previous.clone());
        self.redo.push(undone);
        Some(previous)
    }

    pub fn redo(&mut self, current: &T) -> Option<T> {
        if *current != self.checkpoint {
            return None;
        }
        let next = self.redo.pop()?;
        let redone = std::mem::replace(&mut self.checkpoint, next.clone());
        self.undo.push_back(redone);
        Some(next)
    }

    pub fn can_undo(&self, current: &T) -> bool {
        !self.undo.is_empty() || *current != self.checkpoint
    }

    pub fn can_redo(&self, current: &T) -> bool {
        !self.redo.is_empty() && *current == self.checkpoint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_commit_is_ignored() {
        let mut h = History::new(1, 10);
        assert!(!h.commit(&1));
        assert!(!h.can_undo(&1));
    }

    #[test]
    fn undo_and_redo_walk_checkpoints() {
        let mut h = History::new(0, 10);
        h.commit(&1);
        h.commit(&2);
        assert_eq!(h.undo(&2), Some(1));
        assert_eq!(h.undo(&1), Some(0));
        assert_eq!(h.undo(&0), None);
        assert_eq!(h.redo(&0), Some(1));
        assert_eq!(h.redo(&1), Some(2));
        assert_eq!(h.redo(&2), None);
    }

    #[test]
    fn undo_commits_pending_edit_first() {
        let mut h = History::new(0, 10);
        assert!(h.can_undo(&5));
        assert_eq!(h.undo(&5), Some(0));
        assert_eq!(h.redo(&0), Some(5));
    }

    #[test]
    fn new_commit_clears_redo() {
        let mut h = History::new(0, 10);
        h.commit(&1);
        h.undo(&1);
        h.commit(&7);
        assert!(!h.can_redo(&7));
        assert_eq!(h.redo(&7), None);
    }

    #[test]
    fn redo_refused_after_uncommitted_edit() {
        let mut h = History::new(0, 10);
        h.commit(&1);
        h.undo(&1);
        assert!(!h.can_redo(&3));
        assert_eq!(h.redo(&3), None);
    }

    #[test]
    fn oldest_steps_drop_past_limit() {
        let mut h = History::new(0, 2);
        for v in 1..=4 {
            h.commit(&v);
        }
        assert_eq!(h.undo(&4), Some(3));
        assert_eq!(h.undo(&3), Some(2));
        assert_eq!(h.undo(&2), None);
    }
}
