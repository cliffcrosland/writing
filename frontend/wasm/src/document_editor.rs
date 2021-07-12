use std::fmt::Write;

use js_sys::{Date, JsString};
use wasm_bindgen::prelude::*;
use web_sys::console;

use ot::writing_proto::{ChangeSet, change_op::Op};

const TIME_BETWEEN_HISTORY_COMMIT: f64 = 5000.0;

#[wasm_bindgen]
#[derive(Debug)]
pub struct DocumentEditorModel {
    id: JsString,
    selection: Selection,
    value: JsString,
    history: Vec<ChangeSet>,
    last_change_set_started_at: f64,
}

#[wasm_bindgen]
impl DocumentEditorModel {
    pub fn new(client_id: JsString) -> Self {
        Self {
            id: client_id,
            selection: Default::default(),
            value: JsString::from(""),
            history: Vec::new(),
            last_change_set_started_at: Date::now(),
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
        let mut change_set = ChangeSet::new();
        match &input_event.input_type[..] {
            "historyUndo" => {
                // TODO(cliff): Pop last change set from history, and apply its inverse. All change
                // sets in local memory need to be invertible. That means each deletion needs to
                // keep information about the chars it deleted.
                self.history.pop();
                return;
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

        self.value = match ot::apply_vec_u16(&self.value.iter().collect(), &change_set) {
            Ok(new_value) => JsString::from_char_code(&new_value),
            Err(e) => {
                console::log_1(&format!("ot::apply error: {:?}", e).into());
                "".into()
            }
        };

        self.selection = input_event.selection;
        if self.history.is_empty() {
            self.history.push(change_set);
            return;
        }
        let now = Date::now();
        let elapsed = now - self.last_change_set_started_at;
        if elapsed >= TIME_BETWEEN_HISTORY_COMMIT {
            self.history.push(change_set);
            self.last_change_set_started_at = now;
            return;
        }
        let last_change_set = self.history.last_mut().unwrap();
        match ot::compose(last_change_set, &change_set) {
            Ok(composed) => {
                *last_change_set = composed;
            }
            Err(e) => {
                console::log_1(&format!("ot::compose error: {:?}", e).into());
            }
        }
    }

    #[wasm_bindgen(js_name = getHistory)]
    pub fn get_history(&self) -> JsValue {
        let mut ret = Vec::new();
        for change_set in &self.history {
            ret.push(get_change_set_description(change_set));
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
        selection: Selection
    ) -> Self {
        Self {
            input_type,
            native_event_data,
            target_value,
            selection
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
                let content_str: String = String::from_utf16(&content_u16).unwrap_or_else(|_| "".to_string());
                write!(&mut ret, "Insert(\"{}\")", &content_str).unwrap();
            }
        }
    }
    ret
}
