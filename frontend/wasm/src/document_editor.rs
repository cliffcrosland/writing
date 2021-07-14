use std::fmt::Write;

use js_sys::{Date, JsString};
use wasm_bindgen::prelude::*;
use web_sys::console;

use ot::writing_proto::{change_op::Op, ChangeSet};

const MAX_REVISION_LIFE_TIME: f64 = 2000.0;

#[wasm_bindgen]
#[derive(Debug)]
pub struct DocumentEditorModel {
    id: JsString,
    selection: Selection,
    value: JsString,
    value_at_start_of_last_revision: JsString,
    revisions: Vec<Revision>,
    undo_stack: Vec<UndoRedoItem>,
    redo_stack: Vec<UndoRedoItem>,
}

#[derive(Debug)]
struct Revision {
    change_set: ChangeSet,
    committed_at: f64,
    start_value: JsString,
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
    pub fn new(client_id: JsString) -> Self {
        Self {
            id: client_id,
            selection: Default::default(),
            value: JsString::from(""),
            value_at_start_of_last_revision: JsString::from(""),
            revisions: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    #[wasm_bindgen(js_name = getId)]
    pub fn get_id(&self) -> JsString {
        self.id.clone()
    }

    #[wasm_bindgen(js_name = getSelection)]
    pub fn get_selection(&self) -> Selection {
        self.selection
    }

    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(&mut self, selection: Selection) {
        self.selection = selection;
    }

    #[wasm_bindgen(js_name = getValue)]
    pub fn get_value(&self) -> JsString {
        self.value.clone()
    }

    #[wasm_bindgen(js_name = updateFromInputEvent)]
    pub fn update_from_input_event(&mut self, input_event: InputEventParams) {
        let input_type = &input_event.input_type[..];

        // Handle undo/redo
        let (change_set, selection) = match input_type {
            "historyUndo" => {
                // Undo: Pop from undo stack, push to redo stack, append to revisions.
                let tuple = self.process_undo_redo(UndoRedoType::Undo);
                if tuple.is_none() {
                    return;
                }
                tuple.unwrap()
            }
            "historyRedo" => {
                // Redo: Pop from redo stack, push to undo stack, append to revisions.
                let tuple = self.process_undo_redo(UndoRedoType::Redo);
                if tuple.is_none() {
                    return;
                }
                tuple.unwrap()
            }
            _ => {
                // For all other changes:
                //
                // - Clear redo stack.
                // - If the last revision hasn't been committed yet, update it.
                // - Otherwise, start a new revision.
                self.redo_stack.clear();
                let change_set =
                    compute_change_set_from_input_event(&self.selection, &self.value, &input_event);
                let now = Date::now();
                if !self.revisions.is_empty() && now < self.revisions.last().unwrap().committed_at {
                    let last_revision = &mut self.revisions.last_mut().unwrap();
                    last_revision.change_set =
                        ot::compose(&last_revision.change_set, &change_set).unwrap();
                } else {
                    self.revisions.push(Revision {
                        change_set: change_set.clone(),
                        committed_at: now + MAX_REVISION_LIFE_TIME,
                        start_value: self.value.clone(),
                    });
                    self.undo_stack.push(Default::default());
                }
                let last_revision = &self.revisions.last().unwrap();
                let undo_item = &mut self.undo_stack.last_mut().unwrap();
                undo_item.change_set =
                    invert(&last_revision.start_value, &last_revision.change_set);
                (change_set, input_event.selection)
            }
        };
        self.value = apply(&self.value, &change_set);
        self.selection = selection;
    }

    #[wasm_bindgen(js_name = getRevisions)]
    pub fn get_revisions(&self) -> JsValue {
        let mut ret = Vec::new();
        for revision in &self.revisions {
            ret.push(get_change_set_description(&revision.change_set));
        }
        JsValue::from_serde(&ret).unwrap()
    }

    fn process_undo_redo(
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
            selection: item.selection,
        });
        self.revisions.push(Revision {
            change_set: item.change_set.clone(),
            committed_at: Date::now(),
            start_value: self.value.clone(),
        });
        Some((item.change_set, item.selection))
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
    start: u32,
    end: u32,
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
                    content_u16.push(*ch as u16);
                }
                let content_str: String =
                    String::from_utf16(&content_u16).unwrap_or_else(|_| "".to_string());
                write!(&mut ret, "Insert(\"{}\")", &content_str).unwrap();
            }
        }
    }
    ret
}

fn compute_change_set_from_input_event(
    prior_selection: &Selection,
    prior_value: &JsString,
    input_event: &InputEventParams,
) -> ChangeSet {
    let mut change_set = ChangeSet::new();
    let input_type = &input_event.input_type[..];
    match input_type {
        "deleteByCut" | "deleteByDrag" => {
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
            change_set.retain(
                (input_event.selection.start - input_event.native_event_data.length()).into(),
            );
            change_set.insert_vec(js_string_to_vec_u32(&input_event.native_event_data));
            change_set
                .retain((input_event.target_value.length() - input_event.selection.end).into());
        }
        "insertText" | "insertFromPaste" => {
            change_set.retain(prior_selection.start.into());
            change_set.delete(prior_selection.length().into());
            change_set.insert_vec(js_string_to_vec_u32(&input_event.native_event_data));
            change_set.retain((prior_value.length() - prior_selection.end).into());
        }
        _ => {
            console::warn_1(&format!("Unknown input type: {}", input_type).into());
        }
    }
    change_set
}

fn invert(prior_value: &JsString, change_set: &ChangeSet) -> ChangeSet {
    let prior_value_chars: Vec<u16> = prior_value.iter().collect();
    ot::invert_slice(&prior_value_chars, change_set).unwrap()
}

fn apply(prior_value: &JsString, change_set: &ChangeSet) -> JsString {
    let value_chars: Vec<u16> = prior_value.iter().collect();
    match ot::apply_slice(&value_chars, &change_set) {
        Ok(new_value) => JsString::from_char_code(&new_value),
        Err(e) => {
            console::error_1(&format!("ot::apply error: {:?}", e).into());
            "".into()
        }
    }
}
