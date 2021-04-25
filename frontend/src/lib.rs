use wasm_bindgen::prelude::*;
use yew::events::InputData;
use yew::prelude::*;

use ot::writing_proto::{change_op::Op, ChangeOp, ChangeSet, Delete, Insert, Retain};
use ot::OtError;

struct ClientModel {
    link: ComponentLink<Self>,
    value: String,
    change_set_log: Vec<ChangeSet>,
}

enum Event {
    OnComposeClicked,
    OnInput(InputData),
}

impl Component for ClientModel {
    type Message = Event;
    type Properties = ();
    fn create(_: Self::Properties, link: ComponentLink<Self>) -> Self {
        Self {
            link,
            value: String::new(),
            change_set_log: Vec::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Event::OnInput(InputData { value }) if self.value != value => {
                match get_change_set_from_diff(&self.value, &value) {
                    Ok(change_set) => {
                        self.change_set_log.push(change_set);
                    }
                    Err(e) => {
                        dbg!(e);
                    }
                }
                self.value = value;
                true
            }
            Event::OnComposeClicked => match compose_change_set_log(&self.change_set_log) {
                Ok(composed_change_set_log) => {
                    self.change_set_log = composed_change_set_log;
                    true
                }
                Err(e) => {
                    dbg!(e);
                    false
                }
            },
            _ => false,
        }
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        // Should only return "true" if new properties are different to
        // previously received properties.
        // This component has no properties so we will always return "false".
        false
    }

    fn view(&self) -> Html {
        html! {
            <>
                <div>
                    <textarea oninput=self.link.callback(|input_data| Event::OnInput(input_data))></textarea>
                </div>
                <div>
                    <button onclick=self.link.callback(|_| Event::OnComposeClicked)>{ "Compose Change Sets" }</button>
                </div>
                <div>
                    { self.change_set_log.iter().rev().map(render_change_set).collect::<Html>() }
                </div>
            </>
        }
    }
}

fn render_change_op(change_op: &ChangeOp) -> Html {
    let content = match &change_op.op {
        Some(Op::Retain(retain)) => format!("Retain({})", retain.count),
        Some(Op::Delete(delete)) => format!("Delete({})", delete.count),
        Some(Op::Insert(insert)) => format!("Insert(\"{}\")", &insert.content),
        None => "NONE!".to_string(),
    };
    html! {
        <li> { content } </li>
    }
}

fn render_change_set(change_set: &ChangeSet) -> Html {
    html! {
        <div>
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

    let before: Vec<char> = before.chars().collect();
    let after: Vec<char> = after.chars().collect();
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
        let content = &after[start..(start + len as usize)];
        let content: String = content.iter().cloned().collect();
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

fn compose_change_set_log(change_set_log: &[ChangeSet]) -> Result<Vec<ChangeSet>, OtError> {
    if change_set_log.is_empty() {
        return Ok(vec![]);
    }
    let mut composed_change_set = ChangeSet { ops: Vec::new() };
    for change_set in change_set_log.iter() {
        composed_change_set = ot::compose(&composed_change_set, change_set)?;
    }
    Ok(vec![composed_change_set])
}

#[wasm_bindgen(start)]
pub fn run_app() {
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
