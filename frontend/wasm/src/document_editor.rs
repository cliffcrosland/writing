use std::collections::VecDeque;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

use js_sys::{Date, JsString, Promise};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use web_sys::console;

use ot::writing_proto::{
    change_op::Op, ChangeSet, DocumentRevision, SubmitDocumentChangeSetRequest,
};

use crate::backend_api::BackendApi;

// When a user is typing, their keystrokes will edit the most recent revision. Once the revision is
// a few seconds old, it will be committed to the revision log, and a corresponding undo item will
// be pushed to the undo stack.
//
// Reason: If the user is typing a lot, we don't want each keystroke to create a new revision.
const MAX_REVISION_EDITABLE_TIME: f64 = 2000.0;

#[wasm_bindgen]
pub struct DocumentEditorModel {
    doc_id: String,
    org_id: String,
    user_id: String,
    inner: Arc<Mutex<DocumentEditorModelInner>>,
    api: Arc<BackendApi>,
}

struct DocumentEditorModelInner {
    selection: Selection,
    value: JsString,
    value_before_last_local_revision: JsString,
    committed_revisions: Vec<DocumentRevision>,
    local_revisions: VecDeque<LocalDocumentRevision>,
    submitted_revision_request: Option<SubmitDocumentChangeSetRequest>,
    undo_stack: Vec<UndoRedoItem>,
    redo_stack: Vec<UndoRedoItem>,
}

#[derive(Debug)]
struct LocalDocumentRevision {
    change_set: ChangeSet,
    editable_until: f64,
}

#[derive(Debug, Default)]
struct UndoRedoItem {
    change_set: ChangeSet,
    selection: Selection,
}

enum UndoRedoType {
    Undo,
    Redo,
}

#[wasm_bindgen]
impl DocumentEditorModel {
    pub fn new(org_id: String, doc_id: String, user_id: String) -> Self {
        Self {
            doc_id,
            org_id,
            user_id,
            api: Arc::new(BackendApi::new()),
            inner: Arc::new(Mutex::new(DocumentEditorModelInner {
                selection: Default::default(),
                value: JsString::from(""),
                value_before_last_local_revision: JsString::from(""),
                committed_revisions: Vec::new(),
                local_revisions: VecDeque::new(),
                submitted_revision_request: None,
                undo_stack: Vec::new(),
                redo_stack: Vec::new(),
            })),
        }
    }

    #[wasm_bindgen(js_name = getUserId)]
    pub fn get_user_id(&self) -> String {
        self.user_id.clone()
    }

    #[wasm_bindgen(js_name = getSelection)]
    pub fn get_selection(&self) -> Selection {
        let inner = self.inner.lock().unwrap();
        inner.selection
    }

    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(&mut self, selection: Selection) {
        let mut inner = self.inner.lock().unwrap();
        inner.selection = selection;
    }

    #[wasm_bindgen(js_name = getValue)]
    pub fn get_value(&self) -> JsString {
        let inner = self.inner.lock().unwrap();
        inner.value.clone()
    }

    #[wasm_bindgen(js_name = updateFromInputEvent)]
    pub fn update_from_input_event(&self, input_event: InputEventParams) {
        let mut inner = self.inner.lock().unwrap();
        let input_type = &input_event.input_type[..];

        // Handle undo/redo
        let (change_set, selection) = match input_type {
            "historyUndo" | "historyRedo" => {
                let command = if input_type == "historyUndo" {
                    // Undo: Pop from undo stack, push to redo stack. Update revisions log.
                    UndoRedoType::Undo
                } else {
                    // Redo: Pop from redo stack, push to undo stack. Update revisions log.
                    UndoRedoType::Redo
                };
                let tuple = inner.process_undo_redo_command(command);
                if tuple.is_none() {
                    return;
                }
                tuple.unwrap()
            }
            _ => {
                // For all other edits:
                // - Clear redo stack.
                // - Process the edit, updating the revisions log and undo stack.
                inner.redo_stack.clear();
                inner.process_edit_command(&input_event)
            }
        };
        inner.value = apply(&inner.value, &change_set);
        inner.selection = selection;
    }

    #[wasm_bindgen(js_name = submitNextRevision)]
    pub fn submit_next_revision(&self) -> Promise {
        let request: SubmitDocumentChangeSetRequest;
        {
            let mut inner = self.inner.lock().unwrap();
            let submitted_revision = match inner.local_revisions.pop_front() {
                Some(rev) => rev,
                None => {
                    return Promise::resolve(&JsValue::UNDEFINED);
                }
            };
            request = SubmitDocumentChangeSetRequest {
                doc_id: self.doc_id.clone(),
                org_id: self.org_id.clone(),
                on_revision_number: inner.last_committed_revision_number(),
                change_set: Some(submitted_revision.change_set),
            };

            inner.submitted_revision_request = Some(request.clone());
        }
        let api = Arc::clone(&self.api);
        let inner = Arc::clone(&self.inner);
        let future = async move {
            let mut response = match api.submit_document_change_set(&request).await {
                Ok(response) => response,
                Err(e) => {
                    let error_message: JsValue = format!("API error: {}", e).into();
                    console::error_1(&error_message);
                    return Err(error_message);
                }
            };

            use ot::writing_proto::submit_document_change_set_response::ResponseCode;

            match response.response_code() {
                ResponseCode::Ack => {
                    if response.revisions.len() != 1 {
                        let error_message: JsValue = format!(
                            "Unexpected revisions length in response: {}",
                            response.revisions.len()
                        )
                        .into();
                        console::error_1(&error_message);
                        return Err(error_message);
                    }
                    let committed_revision = response.revisions.pop().unwrap();
                    let mut inner = inner.lock().unwrap();
                    inner.committed_revisions.push(committed_revision);
                    Ok(JsValue::UNDEFINED)
                }
                ResponseCode::DiscoveredNewRevisions => {
                    // TODO(cliff): Implement this case.
                    let error_message: JsValue = "SOMEONE ELSE ADDED REVISIONS!".into();
                    console::error_1(&error_message);
                    Err(error_message)
                }
                _ => {
                    let error_message: JsValue = format!(
                        "API error: Unknown response code {}",
                        response.response_code
                    )
                    .into();
                    console::error_1(&error_message);
                    Err(error_message)
                }
            }
        };
        future_to_promise(future)
    }

    #[wasm_bindgen(js_name = getDebugRevisions)]
    pub fn get_debug_revisions(&self) -> JsValue {
        let inner = self.inner.lock().unwrap();
        let mut ret: Vec<String> = Vec::new();
        for document_revision in &inner.committed_revisions {
            let revision_number = document_revision.revision_number;
            let change_set = document_revision.change_set.as_ref().unwrap();
            let change_set_description = get_change_set_description(change_set);
            ret.push(format!("{}: {}", revision_number, change_set_description));
        }
        if let Some(request) = inner.submitted_revision_request.as_ref() {
            let expected_revision_number = request.on_revision_number + 1;
            let change_set = request.change_set.as_ref().unwrap();
            let change_set_description = get_change_set_description(change_set);
            ret.push(format!(
                "Submitted {}: {}",
                expected_revision_number, change_set_description
            ));
        }
        for local_document_revision in &inner.local_revisions {
            let change_set_description =
                get_change_set_description(&local_document_revision.change_set);
            ret.push(format!("local: {}", change_set_description));
        }
        JsValue::from_serde(&ret).unwrap()
    }

    #[wasm_bindgen(js_name = hasSubmittedRevision)]
    pub fn has_submitted_revision(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.submitted_revision_request.is_some()
    }
}

#[wasm_bindgen]
impl DocumentEditorModelInner {
    fn process_undo_redo_command(
        &mut self,
        undo_redo_type: UndoRedoType,
    ) -> Option<(ChangeSet, Selection)> {
        let (from_stack, to_stack) = match undo_redo_type {
            UndoRedoType::Undo => (&mut self.undo_stack, &mut self.redo_stack),
            UndoRedoType::Redo => (&mut self.redo_stack, &mut self.undo_stack),
        };
        if from_stack.is_empty() {
            return None;
        }
        let item = from_stack.pop().unwrap();
        let inverted_change_set = invert(&self.value, &item.change_set);
        to_stack.push(UndoRedoItem {
            change_set: inverted_change_set,
            selection: self.selection,
        });
        self.push_local_revision(item.change_set.clone(), 0.0);
        Some((item.change_set, item.selection))
    }

    fn process_edit_command(&mut self, input_event: &InputEventParams) -> (ChangeSet, Selection) {
        let (change_set, should_start_new_revision) =
            compute_change_set_from_input_event(&self.selection, &self.value, input_event);
        let editable_until = if should_start_new_revision {
            0.0
        } else {
            Date::now() + MAX_REVISION_EDITABLE_TIME
        };
        if should_start_new_revision || !is_revision_editable(self.local_revisions.back()) {
            self.push_local_revision(change_set.clone(), editable_until);
            self.undo_stack.push(UndoRedoItem {
                change_set: ChangeSet::new(),
                selection: self.selection,
            });
        } else {
            let last_revision = &mut self.local_revisions.back_mut().unwrap();
            last_revision.change_set = match ot::compose(&last_revision.change_set, &change_set) {
                Ok(composed) => composed,
                Err(e) => {
                    let error_message = format!("ot::invert error: {:?}", e);
                    console::error_1(&error_message.clone().into());
                    panic!("{}", error_message);
                }
            };
        }
        let last_revision = &self.local_revisions.back().unwrap();
        let undo_item = &mut self.undo_stack.last_mut().unwrap();
        undo_item.change_set = invert(
            &self.value_before_last_local_revision,
            &last_revision.change_set,
        );
        (change_set, input_event.selection)
    }

    fn push_local_revision(&mut self, change_set: ChangeSet, editable_until: f64) {
        self.value_before_last_local_revision = self.value.clone();
        self.local_revisions.push_back(LocalDocumentRevision {
            change_set,
            editable_until,
        });
    }

    fn last_committed_revision_number(&self) -> i64 {
        match self.committed_revisions.last().as_ref() {
            Some(rev) => rev.revision_number,
            None => 0,
        }
    }
}

#[wasm_bindgen]
#[derive(Debug)]
pub struct InputEventParams {
    input_type: String,
    native_event_data: JsString,
    target_value: JsString,
    selection: Selection,
}

#[wasm_bindgen]
impl InputEventParams {
    pub fn new(
        input_type: String,
        native_event_data: JsString,
        target_value: JsString,
        selection: Selection,
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
pub struct Selection {
    pub start: u32,
    pub end: u32,
}

#[wasm_bindgen]
impl Selection {
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

fn get_change_set_description(change_set: &ChangeSet) -> String {
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
    prior_value: &JsString,
    input_event: &InputEventParams,
) -> (ChangeSet, ShouldStartNewRevision) {
    let mut change_set = ChangeSet::new();
    let input_type = &input_event.input_type[..];
    let mut should_start_new_revision = prior_selection.length() > 0;
    match input_type {
        "deleteByCut" | "deleteByDrag" => {
            should_start_new_revision = true;
            change_set.retain(prior_selection.start.into());
            change_set.delete(prior_selection.length().into());
            change_set.retain((prior_value.length() - prior_selection.end).into());
        }
        "deleteContentBackward" | "deleteContentForward" => {
            if prior_selection.length() > 0 {
                change_set.retain(prior_selection.start.into());
                change_set.delete(prior_selection.length().into());
            } else {
                change_set.retain(input_event.selection.start.into());
                change_set
                    .delete((prior_value.length() - input_event.target_value.length()).into());
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
            change_set.retain((prior_value.length() - prior_selection.end).into());
        }
        "insertLineBreak" => {
            should_start_new_revision = true;
            change_set.retain(prior_selection.start.into());
            change_set.insert("\n");
            change_set.retain((prior_value.length() - prior_selection.end).into());
        }
        _ => {
            let error_message = format!("Unknown input type: {}", input_type);
            console::error_1(&error_message.clone().into());
            panic!("{}", error_message);
        }
    }
    (change_set, should_start_new_revision)
}

fn is_revision_editable(revision: Option<&LocalDocumentRevision>) -> bool {
    match revision {
        None => false,
        Some(revision) => Date::now() < revision.editable_until,
    }
}

fn invert(prior_value: &JsString, change_set: &ChangeSet) -> ChangeSet {
    let prior_value_chars: Vec<u16> = prior_value.iter().collect();
    match ot::invert_slice(&prior_value_chars, change_set) {
        Ok(inverted) => inverted,
        Err(e) => {
            let error_message = format!("ot::invert error: {:?}", e);
            console::error_1(&error_message.clone().into());
            panic!("{}", error_message);
        }
    }
}

fn apply(prior_value: &JsString, change_set: &ChangeSet) -> JsString {
    let prior_value_chars: Vec<u16> = prior_value.iter().collect();
    match ot::apply_slice(&prior_value_chars, &change_set) {
        Ok(new_value) => JsString::from_char_code(&new_value),
        Err(e) => {
            let error_message = format!("ot::apply error: {:?}", e);
            console::error_1(&error_message.clone().into());
            panic!("{}", error_message);
        }
    }
}
