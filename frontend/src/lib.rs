#![recursion_limit = "1024"]

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

use std::collections::VecDeque;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::console;
use web_sys::{Event, HtmlTextAreaElement};
use yew::events::InputData;
use yew::prelude::*;

use ot::writing_proto::{change_op::Op, ChangeOp, ChangeSet, Delete, Insert, Retain};
use ot::OtError;

struct ClientModel {
    link: ComponentLink<Self>,
    value: String,
    local_change_sets: VecDeque<ChangeSet>,
    submitted_change_sets: Vec<ChangeSet>,
}

enum AppEvent {
    ComposeClicked,
    SubmitOneClicked,
    TextAreaInput(InputData),
    TextAreaSelect(Event),
}

impl ClientModel {
    fn update_impl(&mut self, msg: AppEvent) -> ShouldRender {
        match msg {
            AppEvent::TextAreaInput(InputData { value }) if self.value != value => {
                match get_change_set_from_diff(&self.value, &value) {
                    Ok(change_set) => {
                        self.local_change_sets.push_back(change_set);
                    }
                    Err(e) => {
                        log::error!("Error getting change set from diff: {:?}", e);
                    }
                }
                self.value = value;
                true
            }
            AppEvent::TextAreaSelect(event) => {
                let target = match event.target() {
                    Some(target) => target,
                    None => {
                        return false;
                    }
                };
                let text_area = match target.dyn_ref::<HtmlTextAreaElement>() {
                    Some(text_area) => text_area,
                    None => {
                        return false;
                    }
                };
                let selection_start = text_area.selection_start();
                let selection_end = text_area.selection_end();
                log::info!("Selection start: {:?}", selection_start);
                log::info!("Selection end: {:?}", selection_end);
                false
            }
            AppEvent::SubmitOneClicked => {
                if self.local_change_sets.is_empty() {
                    return false;
                }
                let change_set = self.local_change_sets.pop_front().unwrap();
                self.submitted_change_sets.push(change_set);
                true
            }
            AppEvent::ComposeClicked => match compose_change_sets(&self.local_change_sets) {
                Ok(composed_local_change_sets) => {
                    self.local_change_sets = composed_local_change_sets;
                    true
                }
                Err(e) => {
                    log::error!("Error composing local change sets: {:?}", e);
                    false
                }
            },
            _ => false,
        }
    }
}

impl Component for ClientModel {
    type Message = AppEvent;
    type Properties = ();
    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            value: String::new(),
            local_change_sets: VecDeque::new(),
            submitted_change_sets: Vec::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        console::time_with_label("update");
        let should_render = self.update_impl(msg);
        console::time_end_with_label("update");
        should_render
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        // Should only return "true" if new properties are different to
        // previously received properties.
        // This component has no properties so we will always return "false".
        false
    }

    fn view(&self) -> Html {
        html! {
            <div>
                <div>
                    <textarea style="width: 400px; resize: none;"
                              oninput=self.link.callback(|input_data| AppEvent::TextAreaInput(input_data))
                              onselect=self.link.callback(|event| AppEvent::TextAreaSelect(event))>
                    </textarea>
                    <div>
                        { format!("value UTF-16 code point length: {}", &self.value.encode_utf16().count()) }
                    </div>
                </div>
                <div style="display: flex;">
                    <div style="flex: 50%;">
                        <h4>{ "Local Change Sets" }</h4>
                        <div>
                            <button onclick=self.link.callback(|_| AppEvent::SubmitOneClicked)>{ "Submit One" }</button>
                        </div>
                        <div>
                            <button onclick=self.link.callback(|_| AppEvent::ComposeClicked)>{ "Compose All" }</button>
                        </div>
                        {
                            self.local_change_sets
                                .iter()
                                .enumerate()
                                .rev()
                                .map(|(i, change_set)| render_change_set(i + 1, change_set))
                                .collect::<Html>()
                        }
                    </div>
                    <div style="flex: 50%;">
                        <h4>{ "Submitted Change Sets" }</h4>
                        <div>
                            <span>{ "Value: " }</span>
                            <pre>{apply_change_sets("", &self.submitted_change_sets).unwrap()}</pre>
                        </div>
                        {
                            self.submitted_change_sets
                                .iter()
                                .enumerate()
                                .rev()
                                .map(|(i, change_set)| render_change_set(i + 1, change_set))
                                .collect::<Html>()
                        }
                    </div>
                </div>
            </div>
        }
    }
}

fn render_change_op(change_op: &ChangeOp) -> Html {
    let content = match &change_op.op {
        Some(Op::Retain(retain)) => format!("Retain({})", retain.count),
        Some(Op::Delete(delete)) => format!("Delete({})", delete.count),
        Some(Op::Insert(insert)) => {
            let chars: Vec<char> = insert
                .content
                .iter()
                .map(|ch| std::char::from_u32(*ch).unwrap_or(' '))
                .collect();
            format!("Insert({:?})", chars)
        }
        None => "NONE!".to_string(),
    };
    html! {
        <li>{ content }</li>
    }
}

fn render_change_set(revision: usize, change_set: &ChangeSet) -> Html {
    html! {
        <div style="border: 1px solid #ddd;">
            <div>{ format!("Revision: {}", revision) }</div>
            <ul>
                { change_set.ops.iter().map(render_change_op).collect::<Html>() }
            </ul>
        </div>
    }
}

fn get_change_set_from_diff(before: &str, after: &str) -> Result<ChangeSet, OtError> {
    // 1. Compute edit distance matrix.
    struct EditDistanceMatrix {
        num_cols: usize,
        matrix: Vec<usize>,
    }

    impl EditDistanceMatrix {
        fn new(num_rows: usize, num_cols: usize) -> Self {
            Self {
                num_cols,
                matrix: vec![0; num_rows * num_cols],
            }
        }

        fn get(&self, r: usize, c: usize) -> usize {
            self.matrix[r * self.num_cols + c]
        }

        fn set(&mut self, r: usize, c: usize, val: usize) {
            self.matrix[r * self.num_cols + c] = val;
        }
    }

    let before: Vec<u16> = before.encode_utf16().collect();
    let after: Vec<u16> = after.encode_utf16().collect();
    let num_rows = before.len() + 1;
    let num_cols = after.len() + 1;

    let mut edit_dist = EditDistanceMatrix::new(num_rows, num_cols);
    for r in 0..num_rows {
        edit_dist.set(r, 0, r);
    }
    for c in 0..num_cols {
        edit_dist.set(0, c, c);
    }
    for r in 1..num_rows {
        for c in 1..num_cols {
            let delete_dist = edit_dist.get(r - 1, c) + 1;
            let insert_dist = edit_dist.get(r, c - 1) + 1;
            let mut match_dist = edit_dist.get(r - 1, c - 1);
            if before[r - 1] != after[c - 1] {
                match_dist += 1;
            }
            let min_dist = std::cmp::min(match_dist, std::cmp::min(delete_dist, insert_dist));
            edit_dist.set(r, c, min_dist);
        }
    }

    // 2. Trace back through edit distance matrix to see which characters were retained, changed,
    //    deleted, or inserted.
    #[derive(Debug)]
    enum CharOp {
        Insert,
        Delete,
        Change,
        Retain,
    }
    let mut r = num_rows - 1;
    let mut c = num_cols - 1;
    let mut char_ops: Vec<CharOp> = Vec::new();
    char_ops.reserve(num_rows + num_cols);
    while !(r == 0 && c == 0) {
        if r == 0 {
            char_ops.push(CharOp::Insert);
            c -= 1;
        } else if c == 0 {
            char_ops.push(CharOp::Delete);
            r -= 1;
        } else {
            let min_dist = edit_dist.get(r, c);
            let delete_dist = edit_dist.get(r - 1, c) + 1;
            let insert_dist = edit_dist.get(r, c - 1) + 1;
            let change_dist = edit_dist.get(r - 1, c - 1) + 1;

            if min_dist == delete_dist {
                char_ops.push(CharOp::Delete);
                r -= 1;
            } else if min_dist == insert_dist {
                char_ops.push(CharOp::Insert);
                c -= 1;
            } else if min_dist == change_dist {
                char_ops.push(CharOp::Change);
                r -= 1;
                c -= 1;
            } else {
                char_ops.push(CharOp::Retain);
                r -= 1;
                c -= 1;
            }
        }
    }

    // 3. Create a Change Set of OT operations from the character operations.
    let mut change_set = ChangeSet { ops: Vec::new() };
    if char_ops.is_empty() {
        return Ok(change_set);
    }
    let mut c: usize = 0;
    let mut change_start = 0;
    let mut change_len = 0;

    let retain_op = |count| -> Op { Op::Retain(Retain { count }) };
    let delete_op = |count| -> Op { Op::Delete(Delete { count }) };
    let insert_op = |start, len| -> Op {
        let content: Vec<u32> = after[start..(start + len as usize)]
            .iter()
            .map(|ch| *ch as u32)
            .collect();
        Op::Insert(Insert { content })
    };

    for char_op in char_ops.iter().rev() {
        if let CharOp::Change = char_op {
            if change_len == 0 {
                change_start = c;
            }
            change_len += 1;
            c += 1;
            continue;
        }
        if change_len > 0 {
            change_set.push_op(delete_op(change_len))?;
            change_set.push_op(insert_op(change_start, change_len))?;
            change_start = 0;
            change_len = 0;
        }
        match char_op {
            CharOp::Insert => {
                change_set.push_op(insert_op(c, 1))?;
                c += 1;
            }
            CharOp::Delete => {
                change_set.push_op(delete_op(1))?;
            }
            CharOp::Retain => {
                change_set.push_op(retain_op(1))?;
                c += 1;
            }
            _ => {
                unreachable!()
            }
        }
    }
    if change_len > 0 {
        change_set.push_op(delete_op(change_len))?;
        change_set.push_op(insert_op(change_start, change_len))?;
    }

    Ok(change_set)
}

fn compose_change_sets(
    local_change_sets: &VecDeque<ChangeSet>,
) -> Result<VecDeque<ChangeSet>, OtError> {
    if local_change_sets.is_empty() {
        return Ok(VecDeque::new());
    }
    let mut composed_change_set = local_change_sets[0].clone();
    for change_set in local_change_sets.iter().skip(1) {
        composed_change_set = ot::compose(&composed_change_set, change_set)?;
    }
    let mut ret = VecDeque::new();
    if !composed_change_set.ops.is_empty() {
        ret.push_back(composed_change_set);
    }
    Ok(ret)
}

fn apply_change_sets(start: &str, change_sets: &[ChangeSet]) -> Result<String, OtError> {
    let mut doc = start.to_string();
    for change_set in change_sets.iter() {
        doc = ot::apply(&doc, change_set)?;
    }
    Ok(doc)
}

#[wasm_bindgen(start)]
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::default());
    App::<ClientModel>::new().mount_to_body();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_change_set(ops: &[&str]) -> ChangeSet {
        let ops: Vec<ChangeOp> = ops
            .iter()
            .map(|op| {
                if let Some(rest) = op.strip_prefix("I:") {
                    ChangeOp {
                        op: Some(Op::Insert(Insert {
                            content: String::from(rest),
                        })),
                    }
                } else if let Some(rest) = op.strip_prefix("R:") {
                    ChangeOp {
                        op: Some(Op::Retain(Retain {
                            count: rest.parse::<i64>().unwrap(),
                        })),
                    }
                } else if let Some(rest) = op.strip_prefix("D:") {
                    ChangeOp {
                        op: Some(Op::Delete(Delete {
                            count: rest.parse::<i64>().unwrap(),
                        })),
                    }
                } else {
                    unreachable!()
                }
            })
            .collect();
        ChangeSet { ops }
    }

    #[test]
    fn test_get_change_set_from_diff() {
        // Here are the common cases we will need to handle from oninput events:
        // - Insert a character.
        // - Delete a character.
        // - Delete a range of characters.
        // - Delete all characters.

        // Insert a character at end
        let result = get_change_set_from_diff("Hello there", "Hello there!");
        assert_eq!(result.unwrap(), create_change_set(&["R:11", "I:!"]));

        // Insert a character in middle.
        let result = get_change_set_from_diff("Hello there!", "Hello, there!");
        assert_eq!(result.unwrap(), create_change_set(&["R:5", "I:,", "R:7"]));

        // Delete a character.
        let result = get_change_set_from_diff("Hello, there!", "Hello there!");
        assert_eq!(result.unwrap(), create_change_set(&["R:5", "D:1", "R:7"]));

        // Delete a range of characters.
        let result = get_change_set_from_diff("Hello, there!", "Hello!");
        assert_eq!(result.unwrap(), create_change_set(&["R:5", "D:7", "R:1"]));

        // Delete all characters.
        let result = get_change_set_from_diff("Hello!", "");
        assert_eq!(result.unwrap(), create_change_set(&["D:6"]));
    }
}
