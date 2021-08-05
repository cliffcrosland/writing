mod committed_log;
mod pending_log;
mod undo_manager;

use std::collections::HashMap;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

use js_sys::{Date, JsString, Promise};
use thiserror::Error;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use ot::writing_proto::submit_document_change_set_response;
use ot::writing_proto::{change_op::Op, ChangeSet, Selection};
use ot::OtError;

use crate::document_editor::committed_log::CommittedLog;
use crate::document_editor::pending_log::PendingLog;
use crate::document_editor::undo_manager::{UndoItem, UndoManager, UndoType};

// When a user is typing, their keystrokes will edit the most recent revision. Once the revision is
// a few seconds old, it will be committed to the revision log, and a corresponding undo item will
// be pushed to the undo stack.
//
// Reason: If the user is typing a lot, we don't want each keystroke to create a new revision.
const MAX_CURRENT_CHANGE_EDITABLE_TIME: f64 = 2000.0;

#[derive(Debug, Error)]
enum DocumentEditorError {
    #[error("Invalid Input Error: {0}")]
    InvalidInputError(String),
    #[error("Invalid State Error: {0}")]
    InvalidStateError(String),
}

#[wasm_bindgen]
pub struct DocumentEditorModel {
    doc_id: String,
    org_id: String,
    user_id: String,
    inner: Arc<Mutex<DocumentEditorModelInner>>,
}

struct DocumentEditorModelInner {
    committed_log: CommittedLog,
    pending_log: PendingLog,
    undo_manager: UndoManager,
    current_change: Option<CurrentChange>,
    current_selection: Selection,
    sync_running: bool,
}

struct CurrentChange {
    change_set: ChangeSet,
    prior_selection: Selection,
    editable_until: f64,
}

impl CurrentChange {
    fn transform(&mut self, remote: &ChangeSet) -> Result<ChangeSet, OtError> {
        let (transformed_change_set, transformed_remote) = ot::transform(&self.change_set, remote)?;
        self.change_set = transformed_change_set;
        self.prior_selection = ot::transform_selection(&self.prior_selection, &remote)?;
        Ok(transformed_remote)
    }
}

#[wasm_bindgen]
impl DocumentEditorModel {
    pub fn new(org_id: String, doc_id: String, user_id: String) -> Self {
        Self {
            doc_id: doc_id.clone(),
            org_id: org_id.clone(),
            user_id,
            inner: Arc::new(Mutex::new(DocumentEditorModelInner {
                committed_log: CommittedLog::new(&doc_id, &org_id),
                pending_log: PendingLog::new(),
                undo_manager: UndoManager::new(),
                current_change: None,
                current_selection: Selection::default(),
                sync_running: false,
            })),
        }
    }

    #[wasm_bindgen(js_name = getDocId)]
    pub fn get_doc_id(&self) -> String {
        self.doc_id.clone()
    }

    #[wasm_bindgen(js_name = getOrgId)]
    pub fn get_org_id(&self) -> String {
        self.org_id.clone()
    }

    #[wasm_bindgen(js_name = getUserId)]
    pub fn get_user_id(&self) -> String {
        self.user_id.clone()
    }

    #[wasm_bindgen(js_name = getSelection)]
    pub fn get_selection(&self) -> JsSelection {
        let inner = self.inner.lock().unwrap();
        inner.current_selection.clone().into()
    }

    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(&self, selection: JsSelection) {
        let mut inner = self.inner.lock().unwrap();
        inner.current_selection = selection.into();
    }

    #[wasm_bindgen(js_name = computeValue)]
    pub fn compute_value(&self) -> JsString {
        let inner = self.inner.lock().unwrap();
        JsString::from_char_code(&inner.compute_value())
    }

    #[wasm_bindgen(js_name = isSyncRunning)]
    pub fn is_sync_running(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.is_sync_running()
    }

    #[wasm_bindgen(js_name = updateFromInputEvent)]
    pub fn update_from_input_event(&self, input_event: InputEventParams) {
        let mut inner = self.inner.lock().unwrap();
        match inner.update_from_input_event(input_event) {
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
        let inner = self.inner.clone();
        let future = async move {
            let mut inner = inner.lock().unwrap();
            match inner.sync().await {
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
        let inner = self.inner.lock().unwrap();
        let mut ret: Vec<String> = Vec::new();
        ret.append(&mut inner.committed_log.get_debug_lines());
        ret.append(&mut inner.pending_log.get_debug_lines());
        if let Some(current_change) = inner.current_change.as_ref() {
            ret.push(format!(
                "{}, editable until: {}",
                get_change_set_description(&current_change.change_set),
                current_change.editable_until
            ));
        }
        ret.append(&mut inner.undo_manager.get_debug_lines());
        JsValue::from_serde(&ret).unwrap()
    }
}

impl DocumentEditorModelInner {
    pub async fn sync(&mut self) -> anyhow::Result<()> {
        if self.sync_running {
            web_sys::console::warn_1(&JsValue::from_str(
                "Warning: Tried to initiate a new sync, but a sync was already running",
            ));
            return Ok(());
        }
        self.sync_running = true;
        let result = self.sync_impl().await;
        self.sync_running = false;
        result
    }

    async fn sync_impl(&mut self) -> anyhow::Result<()> {
        for _ in 0..2 {
            let mut retry_once = false;
            // 1. Try to commit the next pending local revision to the remote server.
            if let Some(change_set) = self.pending_log.front() {
                use submit_document_change_set_response::ResponseCode;
                match self
                    .committed_log
                    .commit_local_change_set(change_set)
                    .await?
                {
                    ResponseCode::Ack => {
                        self.pending_log.pop_front();
                    }
                    ResponseCode::DiscoveredNewRevisions => {
                        // If we found new remote revisions, we will load them below and try
                        // committing one more time.
                        retry_once = true;
                    }
                    _ => {
                        return Err(DocumentEditorError::InvalidStateError(
                            "Received unknown response code.".to_string(),
                        )
                        .into());
                    }
                }
            }
            // 2. Load any new revisions from the remote server. If we found some, transform
            //    pending local revisions, and transform the current selection.
            match self.committed_log.load_new_remote_revisions().await? {
                None => {
                    return Ok(());
                }
                Some(composed_remote_revisions) => {
                    // Transform pending log.
                    let mut transformed_remote = self
                        .pending_log
                        .transform(&composed_remote_revisions.composed_change_sets)?;

                    // Transform undo/redo stacks.
                    self.undo_manager.transform(&transformed_remote)?;

                    // Transform current change and selection.
                    if let Some(current_change) = self.current_change.as_mut() {
                        transformed_remote = current_change.transform(&transformed_remote)?;
                    }
                    self.current_selection =
                        ot::transform_selection(&self.current_selection, &transformed_remote)?;
                }
            }
            if !retry_once {
                break;
            }
        }
        Ok(())
    }

    pub fn compute_value(&self) -> Vec<u16> {
        // TODO(cliff): Do something smarter than replaying all changes :)
        let committed_composed = self
            .committed_log
            .compose_range(0..self.committed_log.len())
            .unwrap();
        let pending_composed = self
            .pending_log
            .compose_range(0..self.pending_log.len())
            .unwrap();
        let mut composed =
            Self::compose(committed_composed.as_ref(), pending_composed.as_ref()).unwrap();
        if let Some(current_change) = self.current_change.as_ref() {
            composed = Self::compose(composed.as_ref(), Some(&current_change.change_set)).unwrap();
        }
        if let Some(composed) = composed {
            ot::apply_slice(&[], &composed).unwrap()
        } else {
            vec![]
        }
    }

    pub fn is_sync_running(&self) -> bool {
        self.sync_running
    }

    fn compose(a: Option<&ChangeSet>, b: Option<&ChangeSet>) -> Result<Option<ChangeSet>, OtError> {
        match (a, b) {
            (None, None) => Ok(None),
            (Some(a), None) => Ok(Some(a.clone())),
            (None, Some(b)) => Ok(Some(b.clone())),
            (Some(a), Some(b)) => Ok(Some(ot::compose(a, b)?)),
        }
    }

    pub fn update_from_input_event(&mut self, input_event: InputEventParams) -> anyhow::Result<()> {
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
                self.undo_manager.clear(UndoType::Redo);
                self.process_edit_command(&input_event)
            }
        }
    }

    pub fn process_undo_command(&mut self, undo_type: UndoType) -> anyhow::Result<()> {
        let current_change = self.current_change.take();
        self.append_current_change_to_pending_log(current_change)?;

        let undo_item = match self.undo_manager.pop(undo_type) {
            Some(undo_item) => undo_item,
            None => {
                return Ok(());
            }
        };

        let value = self.compute_value();
        let new_undo_item = UndoItem {
            change_set: ot::invert_slice(&value, &undo_item.change_set)?,
            selection_after: self.current_selection.clone(),
        };
        self.pending_log.push_back(&undo_item.change_set);
        match undo_type {
            UndoType::Undo => self.undo_manager.push(UndoType::Redo, new_undo_item),
            UndoType::Redo => self.undo_manager.push(UndoType::Undo, new_undo_item),
        }
        self.current_selection = undo_item.selection_after;

        Ok(())
    }

    pub fn process_edit_command(&mut self, input_event: &InputEventParams) -> anyhow::Result<()> {
        let value = self.compute_value();
        let (change_set, mut should_start_new_revision) = compute_change_set_from_input_event(
            &self.current_selection.clone().into(),
            &value,
            input_event,
        )?;
        if let Some(current_change) = self.current_change.as_ref() {
            should_start_new_revision =
                should_start_new_revision || Date::now() > current_change.editable_until;
        }
        if should_start_new_revision {
            let current_change = self.current_change.take();
            self.append_current_change_to_pending_log(current_change)?;
        }
        if self.current_change.is_none() {
            self.current_change = Some(CurrentChange {
                change_set,
                prior_selection: self.current_selection.clone(),
                editable_until: Date::now() + MAX_CURRENT_CHANGE_EDITABLE_TIME,
            });
        } else {
            let mut current_change = self.current_change.as_mut().unwrap();
            current_change.change_set = ot::compose(&current_change.change_set, &change_set)?;
        }
        self.current_selection = input_event.selection.into();
        Ok(())
    }

    fn append_current_change_to_pending_log(
        &mut self,
        current_change: Option<CurrentChange>,
    ) -> anyhow::Result<()> {
        let current_change = match current_change {
            Some(current_change) => current_change,
            None => {
                return Ok(());
            }
        };
        let value = self.compute_value();
        let undo_item = UndoItem {
            change_set: ot::invert_slice(&value[..], &current_change.change_set)?,
            selection_after: current_change.prior_selection.clone(),
        };
        self.pending_log.push_back(&current_change.change_set);
        self.undo_manager.push(UndoType::Undo, undo_item);
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
    prior_selection: &JsSelection,
    prior_value: &[u16],
    input_event: &InputEventParams,
) -> anyhow::Result<(ChangeSet, ShouldStartNewRevision)> {
    let mut change_set = ChangeSet::new();
    let input_type = &input_event.input_type[..];
    let mut should_start_new_revision = prior_selection.length() > 0;
    match input_type {
        "deleteByCut" | "deleteByDrag" => {
            should_start_new_revision = true;
            change_set.retain(prior_selection.start.into());
            change_set.delete(prior_selection.length().into());
            change_set.retain((prior_value.len() as u32 - prior_selection.end).into());
        }
        "deleteContentBackward" | "deleteContentForward" => {
            if prior_selection.length() > 0 {
                change_set.retain(prior_selection.start.into());
                change_set.delete(prior_selection.length().into());
            } else {
                change_set.retain(input_event.selection.start.into());
                change_set
                    .delete((prior_value.len() as u32 - input_event.target_value.length()).into());
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
            change_set.retain((prior_value.len() as u32 - prior_selection.end).into());
        }
        "insertLineBreak" => {
            should_start_new_revision = true;
            change_set.retain(prior_selection.start.into());
            change_set.insert("\n");
            change_set.retain((prior_value.len() as u32 - prior_selection.end).into());
        }
        _ => {
            let error_message = format!("Unknown input type: {}", input_type);
            return Err(DocumentEditorError::InvalidInputError(error_message).into());
        }
    }
    Ok((change_set, should_start_new_revision))
}
