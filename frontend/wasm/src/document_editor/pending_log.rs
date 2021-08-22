use std::collections::VecDeque;
use std::ops::Range;

use ot::writing_proto::ChangeSet;
use ot::OtError;

use crate::document_editor::get_change_set_description;

pub struct PendingLog {
    change_sets: VecDeque<ChangeSet>,
}

impl PendingLog {
    pub fn new() -> Self {
        Self {
            change_sets: VecDeque::new(),
        }
    }

    pub fn front(&self) -> Option<&ChangeSet> {
        self.change_sets.front()
    }

    pub fn push_back(&mut self, change_set: &ChangeSet) {
        self.change_sets.push_back(change_set.clone());
    }

    pub fn pop_front(&mut self) -> Option<ChangeSet> {
        self.change_sets.pop_front()
    }

    pub fn back_mut(&mut self) -> Option<&mut ChangeSet> {
        self.change_sets.back_mut()
    }

    pub fn len(&self) -> usize {
        self.change_sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn compress(&mut self) -> Result<(), OtError> {
        if self.change_sets.is_empty() {
            return Ok(());
        }
        let change_set = ot::compose_iter(self.change_sets.iter())?;
        self.change_sets.clear();
        self.change_sets.push_back(change_set);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn compose_range(&self, range: Range<usize>) -> Result<Option<ChangeSet>, OtError> {
        if range.start >= self.change_sets.len() {
            return Ok(None);
        }
        if range.end <= range.start {
            return Ok(None);
        }
        let iter = self.change_sets.range(range);
        Ok(Some(ot::compose_iter(iter)?))
    }

    /// Transforms the local pending changes against the remote change.
    ///
    /// Returns the remote change transformed against the local pending changes.
    ///
    /// In particular, if we have local changes `L1, L2, ..., LN`, and remote change `R`, then this
    /// function transforms the local changes and remote change into `L1', L2', ..., LN'` and `R'`
    /// respectively such that `R * L1' * L2' * ... * LN' == L1 * L2 * ... LN * R'`.
    ///
    /// The function updates the local changes in the pending log to `L1', L2', ..., LN'` and
    /// returns `R'`.
    pub fn transform(&mut self, remote: &ChangeSet) -> Result<ChangeSet, OtError> {
        let mut remote = remote.clone();
        for change_set in self.change_sets.iter_mut() {
            let (transformed_change_set, transformed_remote) = ot::transform(&change_set, &remote)?;
            *change_set = transformed_change_set;
            remote = transformed_remote;
        }
        Ok(remote)
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let mut ret = Vec::new();
        for change_set in self.change_sets.iter() {
            ret.push(format!(
                "local_revision: {}",
                get_change_set_description(change_set)
            ));
        }
        ret
    }
}
