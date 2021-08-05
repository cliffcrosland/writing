use std::collections::VecDeque;

use ot::writing_proto::{ChangeSet, Selection};
use ot::OtError;

use crate::document_editor::get_change_set_description;

const MAX_UNDO_HISTORY_LENGTH: usize = 10_000;

pub struct UndoManager {
    undo_stack: VecDeque<UndoItem>,
    redo_stack: VecDeque<UndoItem>,
}

pub struct UndoItem {
    pub change_set: ChangeSet,
    pub selection_after: Selection,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum UndoType {
    Undo,
    Redo,
}

impl UndoManager {
    pub fn new() -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
        }
    }

    pub fn push(&mut self, undo_type: UndoType, undo_item: UndoItem) {
        let stack = match undo_type {
            UndoType::Undo => &mut self.undo_stack,
            UndoType::Redo => &mut self.redo_stack,
        };
        stack.push_back(undo_item);
        if undo_type == UndoType::Undo && stack.len() > MAX_UNDO_HISTORY_LENGTH {
            stack.pop_front();
        }
    }

    pub fn pop(&mut self, undo_type: UndoType) -> Option<UndoItem> {
        match undo_type {
            UndoType::Undo => self.undo_stack.pop_back(),
            UndoType::Redo => self.redo_stack.pop_back(),
        }
    }

    pub fn transform(&mut self, remote: &ChangeSet) -> Result<(), OtError> {
        Self::transform_stack(&mut self.undo_stack, remote)?;
        Self::transform_stack(&mut self.redo_stack, remote)?;
        Ok(())
    }

    pub fn clear(&mut self, undo_type: UndoType) {
        match undo_type {
            UndoType::Undo => self.undo_stack.clear(),
            UndoType::Redo => self.redo_stack.clear(),
        }
    }

    fn transform_stack(stack: &mut VecDeque<UndoItem>, remote: &ChangeSet) -> Result<(), OtError> {
        let mut remote = remote.clone();
        for undo_item in stack.iter_mut().rev() {
            let (transformed_undo, transformed_remote) =
                ot::transform(&undo_item.change_set, &remote)?;
            let transformed_selection_after =
                ot::transform_selection(&undo_item.selection_after, &remote)?;
            undo_item.change_set = transformed_undo;
            undo_item.selection_after = transformed_selection_after;
            remote = transformed_remote;
        }
        Ok(())
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let mut ret = vec!["Undo Stack (top->bottom)".to_string()];
        for undo_item in self.undo_stack.iter().rev() {
            ret.push(format!(
                "{}, {:?}",
                &get_change_set_description(&undo_item.change_set),
                &undo_item.selection_after,
            ));
        }
        ret.push("Redo Stack (top->bottom)".to_string());
        for undo_item in self.redo_stack.iter().rev() {
            ret.push(format!(
                "{}, {:?}",
                &get_change_set_description(&undo_item.change_set),
                &undo_item.selection_after,
            ));
        }
        ret
    }
}
