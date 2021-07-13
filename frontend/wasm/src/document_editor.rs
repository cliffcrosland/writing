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
    undo_log: Vec<ChangeSet>,
}

#[derive(Debug)]
struct Revision {
    change_set: ChangeSet,
    created_at: f64,
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
            undo_log: Vec::new(),
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

    #[wasm_bindgen(js_name = processInputEvent)]
    pub fn process_input_event(&mut self, input_event: InputEventParams) {
        let now = Date::now();
        let mut change_set = ChangeSet::new();
        match &input_event.input_type[..] {
            "historyUndo" => {
                console::log_1(&format!("UNDO! undo_log len: {}", self.undo_log.len()).into());
                if let Some(undo_change_set) = self.undo_log.pop() {
                    change_set = undo_change_set;
                }
            }
            "historyRedo" => {
                return;
            }
            "deleteByCut" | "deleteByDrag" => {
                change_set.retain(self.selection.start.into());
                change_set.delete(self.selection.length().into());
                change_set.retain((self.value.length() - self.selection.end).into());
            }
            "deleteContentBackward" | "deleteContentForward" => {
                if self.selection.length() > 0 {
                    change_set.retain(self.selection.start.into());
                    change_set.delete(self.selection.length().into());
                    change_set.retain(
                        (input_event.target_value.length() - input_event.selection.end).into(),
                    );
                } else {
                    change_set.retain(input_event.selection.start.into());
                    change_set
                        .delete((self.value.length() - input_event.target_value.length()).into());
                    change_set.retain(
                        (input_event.target_value.length() - input_event.selection.end).into(),
                    );
                }
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
                change_set.retain(self.selection.start.into());
                change_set.delete(self.selection.length().into());
                change_set.insert_vec(js_string_to_vec_u32(&input_event.native_event_data));
                change_set.retain((self.value.length() - self.selection.end).into());
            }
            _ => {
                println!("?");
                return;
            }
        }

        let value_chars: Vec<u16> = self.value.iter().collect();
        let new_value = match ot::apply_slice(&value_chars, &change_set) {
            Ok(new_value) => JsString::from_char_code(&new_value),
            Err(e) => {
                console::log_1(&format!("ot::apply error: {:?}", e).into());
                "".into()
            }
        };

        let should_start_new_revision = self.revisions.is_empty()
            || now - self.revisions.last().unwrap().created_at >= MAX_REVISION_LIFE_TIME;

        if should_start_new_revision {
            // Start a new revision.
            self.revisions.push(Revision {
                change_set,
                created_at: now,
            });
            self.value_at_start_of_last_revision = self.value.clone();
            self.undo_log.push(ChangeSet::new());
        } else {
            // Update most recent revision.
            let last_revision = &mut self.revisions.last_mut().unwrap();
            match ot::compose(&last_revision.change_set, &change_set) {
                Ok(composed) => {
                    last_revision.change_set = composed;
                }
                Err(e) => {
                    console::log_1(&format!("ot::compose error: {:?}", e).into());
                }
            }
        }

        let value_at_start_of_last_revision_chars: Vec<u16> =
            self.value_at_start_of_last_revision.iter().collect();
        let last_revision_change_set = &self.revisions.last().unwrap().change_set;
        match ot::invert_slice(
            &value_at_start_of_last_revision_chars,
            &last_revision_change_set,
        ) {
            Ok(undo_change_set) => {
                self.undo_log.pop();
                self.undo_log.push(undo_change_set);
            }
            Err(e) => {
                console::log_1(&format!("ot::invert error: {:?}", e).into());
                return;
            }
        }

        self.selection = input_event.selection;
        self.value = new_value;
    }

    #[wasm_bindgen(js_name = getRevisions)]
    pub fn get_revisions(&self) -> JsValue {
        let mut ret = Vec::new();
        for revision in &self.revisions {
            ret.push(get_change_set_description(&revision.change_set));
        }
        JsValue::from_serde(&ret).unwrap()
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
        self.clone()
    }

    #[wasm_bindgen(js_name = toString)]
    pub fn to_string(&self) -> String {
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
