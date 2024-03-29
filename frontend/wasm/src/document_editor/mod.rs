mod committed_log;
mod document_value;
mod pending_log;
mod undo_manager;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write;
use std::rc::Rc;

use js_sys::{Date, JsString, Promise};
use thiserror::Error;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use ot::writing_proto::submit_document_change_set_response::ResponseCode;
use ot::writing_proto::{change_op::Op, ChangeSet, Selection};

use crate::document_editor::committed_log::CommittedLog;
use crate::document_editor::document_value::{
    DocumentValue, DocumentValueChunkId, DocumentValueChunkVersion,
};
use crate::document_editor::pending_log::PendingLog;
use crate::document_editor::undo_manager::{UndoItem, UndoManager, UndoType};

// When a user is typing, their keystrokes will edit the most recent revision. Once the revision is
// a few seconds old, it will be committed to the revision log, and a corresponding undo item will
// be pushed to the undo stack.
//
// Reason: If the user is typing a lot, we don't want each keystroke to create a new revision.
const MAX_COMPOSABLE_TIME: f64 = 2000.0;

#[derive(Debug, Error)]
enum DocumentEditorError {
    #[error("Invalid Input Error: {0}")]
    InvalidInputError(String),
    #[error("Invalid State Error: {0}")]
    InvalidStateError(String),
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct DocumentEditorModel {
    inner: Rc<RefCell<DocumentEditorModelInner>>,
}

struct DocumentEditorModelInner {
    doc_id: String,
    committed_log: CommittedLog,
    pending_log: PendingLog,
    undo_manager: UndoManager,
    current_selection: Selection,
    current_value: DocumentValue,
    sync_running: bool,
    last_pending_composable_until: f64,
}

#[wasm_bindgen]
impl DocumentEditorModel {
    pub fn new(doc_id: String) -> Self {
        Self {
            inner: Rc::new(RefCell::new(DocumentEditorModelInner {
                doc_id: doc_id.clone(),
                committed_log: CommittedLog::new(&doc_id),
                pending_log: PendingLog::new(),
                undo_manager: UndoManager::new(),
                current_selection: Selection::default(),
                current_value: DocumentValue::new(),
                sync_running: false,
                last_pending_composable_until: 0.0,
            })),
        }
    }

    #[wasm_bindgen(js_name = getDocId)]
    pub fn get_doc_id(&self) -> String {
        self.inner.borrow().doc_id.clone()
    }

    #[wasm_bindgen(js_name = getSelection)]
    pub fn get_selection(&self) -> JsSelection {
        self.inner.borrow().current_selection.clone().into()
    }

    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(&self, selection: JsSelection) {
        let mut self_ = self.inner.borrow_mut();
        self_.current_selection = selection.into();
    }

    #[wasm_bindgen(js_name = getValue)]
    pub fn get_value(&self) -> JsString {
        let self_ = self.inner.borrow();
        let current_value = self_
            .current_value
            .get_value_in_range(0..self_.current_value.value_len())
            .unwrap();
        slice_to_js_string(&current_value)
    }

    #[wasm_bindgen(js_name = getChunkIds)]
    pub fn get_chunk_ids(&self) -> Vec<DocumentValueChunkId> {
        let self_ = self.inner.borrow();
        self_.current_value.get_chunk_ids()
    }

    #[wasm_bindgen(js_name = getChunkVersions)]
    pub fn get_chunk_versions(&self) -> Vec<DocumentValueChunkVersion> {
        let self_ = self.inner.borrow();
        self_.current_value.get_chunk_versions()
    }

    #[wasm_bindgen(js_name = getChunkValue)]
    pub fn get_chunk_value(&self, id: DocumentValueChunkId) -> JsValue {
        let self_ = self.inner.borrow();
        match self_.current_value.get_chunk(id) {
            None => JsValue::NULL,
            Some(chunk) => slice_to_js_string(&chunk.value).into(),
        }
    }

    #[wasm_bindgen(js_name = isSyncRunning)]
    pub fn is_sync_running(&self) -> bool {
        self.inner.borrow().sync_running
    }

    fn set_sync_running(&self, sync_running: bool) {
        let mut self_ = self.inner.borrow_mut();
        self_.sync_running = sync_running;
    }

    #[wasm_bindgen(js_name = updateFromInputEvent)]
    pub fn update_from_input_event(&self, input_event: InputEventParams) {
        match self.update_from_input_event_impl(input_event) {
            Ok(_) => {}
            Err(e) => {
                web_sys::console::error_1(
                    &format!("Error occurred updating from input event: {}", e).into(),
                );
            }
        }
    }

    #[wasm_bindgen(js_name = sync)]
    pub fn sync(&self) -> Promise {
        let self_ = self.clone();
        let future = async move {
            match self_.sync_impl().await {
                Ok(_) => Ok(JsValue::UNDEFINED),
                Err(e) => {
                    let error_message = format!("Document Editor sync error: {:?}", e);
                    let mut map = HashMap::new();
                    map.insert("error".to_string(), error_message);
                    Err(JsValue::from_serde(&map).unwrap())
                }
            }
        };
        future_to_promise(future)
    }

    #[wasm_bindgen(js_name = getDebugLines)]
    pub fn get_debug_lines(&self) -> JsValue {
        let self_ = self.inner.borrow();
        let mut ret: Vec<String> = Vec::new();
        ret.append(&mut self_.committed_log.get_debug_lines());
        ret.append(&mut self_.pending_log.get_debug_lines());
        ret.append(&mut self_.undo_manager.get_debug_lines());
        JsValue::from_serde(&ret).unwrap()
    }

    async fn sync_impl(&self) -> anyhow::Result<()> {
        if self.is_sync_running() {
            return Ok(());
        }
        self.compress_pending_log()?;
        self.set_sync_running(true);
        let self_ = self.clone();
        let result = self_.run_sync_round().await;
        self_.set_sync_running(false);
        result
    }

    async fn run_sync_round(&self) -> anyhow::Result<()> {
        let self_ = self.clone();
        let pending_log_len = self_.inner.borrow().pending_log.len();
        if pending_log_len == 0 {
            self_.load_new_remote_revisions().await?;
            return Ok(());
        }
        let mut loaded_remote = false;
        for _ in 0..pending_log_len {
            // Try to commit next pending revision. If we could not commit it because we discovered
            // new remote revisions, load the remote revisions and try again only once. Conflict
            // will be resolved eventually in a future sync round.
            match self_.try_commit_next_pending_revision().await? {
                ResponseCode::DiscoveredNewRevisions => {
                    self_.load_new_remote_revisions().await?;
                    self_.try_commit_next_pending_revision().await?;
                    loaded_remote = true;
                }
                _ => continue,
            }
        }
        if !loaded_remote {
            self_.load_new_remote_revisions().await?;
        }
        Ok(())
    }

    async fn try_commit_next_pending_revision(&self) -> anyhow::Result<ResponseCode> {
        if self.inner.borrow().pending_log.front().is_none() {
            return Ok(ResponseCode::Ack);
        }
        let change_set = self.inner.borrow().pending_log.front().unwrap().clone();
        let self_ = self.clone();
        let committed_log = self_.inner.borrow().committed_log.clone();
        match committed_log.commit_local_change_set(&change_set).await? {
            ResponseCode::Ack => {
                self_.inner.borrow_mut().pending_log.pop_front();
                Ok(ResponseCode::Ack)
            }
            ResponseCode::DiscoveredNewRevisions => Ok(ResponseCode::DiscoveredNewRevisions),
            _ => Err(DocumentEditorError::InvalidStateError(
                "Received unknown response code.".to_string(),
            )
            .into()),
        }
    }

    async fn load_new_remote_revisions(&self) -> anyhow::Result<()> {
        let self_ = self.clone();
        let committed_log = self.inner.borrow().committed_log.clone();
        match committed_log.load_new_remote_revisions().await? {
            None => Ok(()),
            Some(composed_remote_revisions) => {
                let mut self_ = self_.inner.borrow_mut();
                // Transform pending log.
                let transformed_remote = self_
                    .pending_log
                    .transform(&composed_remote_revisions.composed_change_sets)?;

                // Apply transformed remote change set to current value.
                self_.current_value.apply(&transformed_remote)?;

                // Transform undo/redo stacks.
                self_.undo_manager.transform(&transformed_remote)?;

                // Transform current change and selection.
                self_.current_selection =
                    ot::transform_selection(&self_.current_selection, &transformed_remote)?;
                Ok(())
            }
        }
    }

    fn update_from_input_event_impl(&self, input_event: InputEventParams) -> anyhow::Result<()> {
        let input_type = &input_event.input_type[..];
        match input_type {
            // Handle undo/redo
            "historyUndo" | "historyRedo" => {
                let undo_type = if input_type == "historyUndo" {
                    UndoType::Undo
                } else {
                    UndoType::Redo
                };
                self.process_undo_command(undo_type)
            }
            _ => {
                // For all other edits:
                // - Clear redo stack.
                // - Process the edit, updating the revisions log and undo stack.
                self.inner.borrow_mut().undo_manager.clear(UndoType::Redo);
                self.process_edit_command(&input_event)
            }
        }
    }

    fn process_undo_command(&self, undo_type: UndoType) -> anyhow::Result<()> {
        let mut self_ = self.inner.borrow_mut();

        let undo_item = match self_.undo_manager.pop(undo_type) {
            Some(undo_item) => undo_item,
            None => {
                return Ok(());
            }
        };

        let new_undo_item = UndoItem {
            change_set: self_.current_value.invert(&undo_item.change_set)?,
            selection_after: self_.current_selection.clone(),
        };
        self_.pending_log.push_back(&undo_item.change_set);
        match undo_type {
            UndoType::Undo => self_.undo_manager.push(UndoType::Redo, new_undo_item),
            UndoType::Redo => self_.undo_manager.push(UndoType::Undo, new_undo_item),
        }
        self_.current_value.apply(&undo_item.change_set)?;
        self_.current_selection = undo_item.selection_after;

        Ok(())
    }

    fn process_edit_command(&self, input_event: &InputEventParams) -> anyhow::Result<()> {
        let mut self_ = self.inner.borrow_mut();
        let (change_set, should_start_new_revision) = compute_change_set_from_input_event(
            &self_.current_selection,
            self_.current_value.value_len() as u32,
            input_event,
        )?;
        let should_start_new_revision = should_start_new_revision
            || self_.pending_log.is_empty()
            || Date::now() > self_.last_pending_composable_until;
        let inverted_change_set = self_.current_value.invert(&change_set)?;
        if should_start_new_revision {
            self_.pending_log.push_back(&change_set);
            let selection_after = self_.current_selection.clone();
            self_.undo_manager.push(
                UndoType::Undo,
                UndoItem {
                    change_set: inverted_change_set,
                    selection_after: selection_after,
                },
            );
            self_.last_pending_composable_until = Date::now() + MAX_COMPOSABLE_TIME;
        } else {
            let last_pending_change_set = self_.pending_log.back_mut().ok_or_else(|| {
                DocumentEditorError::InvalidStateError(String::from("Unexpected empty pending log"))
            })?;
            *last_pending_change_set = ot::compose(&last_pending_change_set, &change_set)?;
            let mut undo_item = self_.undo_manager.pop(UndoType::Undo).ok_or_else(|| {
                DocumentEditorError::InvalidStateError(String::from("Unexpected empty undo stack"))
            })?;
            undo_item.change_set = ot::compose(&inverted_change_set, &undo_item.change_set)?;
            self_.undo_manager.push(UndoType::Undo, undo_item);
        }
        self_.current_value.apply(&change_set)?;
        self_.current_selection = input_event.selection.into();
        Ok(())
    }

    fn compress_pending_log(&self) -> anyhow::Result<()> {
        let mut self_ = self.inner.borrow_mut();
        self_.pending_log.compress()?;
        Ok(())
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct InputEventParams {
    input_type: String,
    native_event_data: JsString,
    target_value: JsString,
    selection: JsSelection,
}

#[wasm_bindgen]
impl InputEventParams {
    pub fn new(
        input_type: String,
        native_event_data: JsString,
        target_value: JsString,
        selection: JsSelection,
    ) -> Self {
        Self {
            input_type,
            native_event_data,
            target_value,
            selection,
        }
    }
}

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, Default)]
pub struct JsSelection {
    pub start: u32,
    pub end: u32,
}

impl From<Selection> for JsSelection {
    fn from(sel: Selection) -> JsSelection {
        JsSelection {
            start: sel.offset as u32,
            end: (sel.offset + sel.count) as u32,
        }
    }
}

impl From<JsSelection> for Selection {
    fn from(sel: JsSelection) -> Selection {
        Selection {
            offset: sel.start as i64,
            count: (sel.end - sel.start) as i64,
        }
    }
}

#[wasm_bindgen]
impl JsSelection {
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub fn length(&self) -> u32 {
        self.end - self.start
    }

    pub fn clone_selection(&self) -> Self {
        *self
    }

    #[wasm_bindgen(js_name = toString)]
    pub fn string(&self) -> String {
        format!("{:?}", self)
    }
}

fn js_string_to_vec_u32(js_string: &JsString) -> Vec<u32> {
    let mut ret = Vec::new();
    for ch in js_string.iter() {
        ret.push(ch as u32);
    }
    ret
}

pub fn get_change_set_description(change_set: &ChangeSet) -> String {
    let mut ret = String::new();
    let mut is_first = true;
    for change_op in &change_set.ops {
        if change_op.op.is_none() {
            continue;
        }
        if is_first {
            is_first = false;
        } else {
            ret.push_str(", ");
        }
        let op = change_op.op.as_ref().unwrap();
        match op {
            Op::Retain(retain) => {
                write!(&mut ret, "Retain({})", retain.count).unwrap();
            }
            Op::Delete(delete) => {
                write!(&mut ret, "Delete({})", delete.count).unwrap();
            }
            Op::Insert(insert) => {
                let mut content_u16: Vec<u16> = Vec::new();
                for ch in &insert.content {
                    let ch = *ch as u16;
                    if ch == '\n' as u16 {
                        content_u16.push('\\' as u16);
                        content_u16.push('n' as u16);
                    } else {
                        content_u16.push(ch);
                    }
                }
                let content_str: String =
                    String::from_utf16(&content_u16).unwrap_or_else(|_| "".to_string());
                if content_str == "\\n" {
                    write!(&mut ret, "Insert('\\n')").unwrap();
                } else {
                    write!(&mut ret, "Insert(\"{}\")", &content_str).unwrap();
                }
            }
        }
    }
    ret
}

type ShouldStartNewRevision = bool;

fn compute_change_set_from_input_event(
    prior_selection: &Selection,
    prior_value_len: u32,
    input_event: &InputEventParams,
) -> anyhow::Result<(ChangeSet, ShouldStartNewRevision)> {
    let prior_selection: JsSelection = prior_selection.clone().into();
    let mut change_set = ChangeSet::with_capacity(4);
    let input_type = &input_event.input_type[..];
    let mut should_start_new_revision = prior_selection.length() > 0;
    match input_type {
        "deleteByCut" | "deleteByDrag" => {
            should_start_new_revision = true;
            change_set.retain(prior_selection.start.into());
            change_set.delete(prior_selection.length().into());
            change_set.retain((prior_value_len - prior_selection.end).into());
        }
        "deleteContentBackward" | "deleteContentForward" => {
            if prior_selection.length() > 0 {
                change_set.retain(prior_selection.start.into());
                change_set.delete(prior_selection.length().into());
            } else {
                change_set.retain(input_event.selection.start.into());
                change_set.delete((prior_value_len - input_event.target_value.length()).into());
            }
            change_set
                .retain((input_event.target_value.length() - input_event.selection.end).into());
        }
        "insertFromDrop" => {
            should_start_new_revision = true;
            change_set.retain(
                (input_event.selection.start - input_event.native_event_data.length()).into(),
            );
            change_set.insert_vec(js_string_to_vec_u32(&input_event.native_event_data));
            change_set
                .retain((input_event.target_value.length() - input_event.selection.end).into());
        }
        "insertText" | "insertFromPaste" => {
            should_start_new_revision = input_type == "insertFromPaste";
            change_set.retain(prior_selection.start.into());
            change_set.delete(prior_selection.length().into());
            change_set.insert_vec(js_string_to_vec_u32(&input_event.native_event_data));
            change_set.retain((prior_value_len - prior_selection.end).into());
        }
        "insertLineBreak" => {
            should_start_new_revision = true;
            change_set.retain(prior_selection.start.into());
            change_set.insert("\n");
            change_set.retain((prior_value_len - prior_selection.end).into());
        }
        _ => {
            let error_message = format!("Unknown input type: {}", input_type);
            return Err(DocumentEditorError::InvalidInputError(error_message).into());
        }
    }
    Ok((change_set, should_start_new_revision))
}

fn slice_to_js_string(value: &[u16]) -> JsString {
    if value.len() < (1usize << 16) {
        JsString::from_char_code(value)
    } else {
        let mut ret = JsString::from("");
        for chunk in value.chunks(1usize << 16) {
            ret = ret.concat(&JsString::from_char_code(chunk));
        }
        ret
    }
}
