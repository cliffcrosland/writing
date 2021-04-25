mod proto;

pub use proto::writing as writing_proto;

use writing_proto::{change_op::Op, ChangeOp, ChangeSet, Delete, Insert, Retain, Selection};

#[derive(Debug)]
pub enum OtError {
    InvalidInput(String),
    PostConditionFailed(String),
}

/// Given a remote change set and a local change set based on the same version of a document,
/// transforms the local change set so that the local changes may be applied after the remote
/// changes.
///
/// # Formal definition of transformation
///
/// A change set can be thought of as a function `f(x) -> y` that takes a document of length `x` as
/// input and returns a document of length `y` as output.
///
/// Let the remote change set be `r(x) -> y`, and let the local change set be `l(x) -> z`. Note
/// that both of the change sets take the same document length `x` as input.
///
/// We transform the function `l` to create a new function `l'(y) -> w` such that `l'(r(x))`
/// returns a document that includes all of the character changes from `l` and `r`. That is:
/// - Every character retained in both `l(x)` and `r(x)` will be retained in `l'(r(x))`.
/// - Every character inserted in either `l(x)` or `r(x)` will be inserted in `l'(r(x))`.
/// - Every character deleted in either `l(x)` or `r(x)` will be deleted in `l'(r(x))`.
///
/// # How local operations are transformed
///
/// Here are all of the ways that operations in the local change set can be transformed:
///
/// - Local `Insert` operations are added verbatim to the transformed local change set.
///
/// - If a local `Retain` overlaps with a remote `Delete`, the length of the local `Retain`
/// decreases by the length of the overlap. No need to retain characters that were deleted
/// remotely.
///
/// - If a local `Retain` overlaps with a remote `Insert`, the length of the local `Retain`
/// increases by the length of the inserted content. We need to retain characters that were
/// inserted remotely.
///
/// - If a local `Delete` overlaps with a remote `Delete`, we do not include the remotely deleted
/// characters in the local `Delete`. No need to delete characters that were deleted remotely.
///
/// - If a local `Delete` overlaps with a remote `Insert`, we need to split the local `Delete` into
/// three operations: `Delete` before the `Insert`, `Retain` the length of the `Insert`, and
/// `Delete` after the `Insert`. We need to retain characters that were inserted remotely.
///
/// - Any trailing `Insert` operations in the remote change set become one contiguous `Retain` in
/// the transformed local change set. We need to retain characters that were inserted remotely.
///
/// # Errors
///
/// - Returns `OtError::InvalidInput` when:
///   - The local and remote change sets have different input document lengths (i.e. We receive
///   arguments `r(x) -> y` and `l(p) -> q` where `x != p`),
///   - A change set contains an empty op.
///   - A change set seems malformed.
///
/// - Returns `OtError::PostConditionFailed`. when we create a transformed local change set that
/// has a different input document length than the updated document length of the remote change
/// set. This means that the transformed local change set cannot be applied after the remote change
/// set, which is a problem. (i.e. Given `r(x) -> y` and `l(x) -> z`, we created an invalid
/// transformed local change set `l'(p) -> q` where `y != p`).
///
pub fn transform(
    remote_change_set: &ChangeSet,
    local_change_set: &ChangeSet,
) -> Result<ChangeSet, OtError> {
    let (remote_len_before, remote_len_after) = get_input_output_doc_lengths(remote_change_set)?;
    let (local_len_before, _) = get_input_output_doc_lengths(local_change_set)?;

    if remote_len_before != local_len_before {
        return Err(OtError::InvalidInput(format!(
            "Both the remote change set and the local change sets must be based on a document of \
            the same length. Remote input document length: {}, Local input document length: {}",
            remote_len_before, local_len_before
        )));
    }

    let unexpected_empty_op_error =
        |name: &str| OtError::InvalidInput(format!("{} change set contained an empty op", name));

    // For each local op, we advance through the remote ops until we have found all remote ops that
    // overlap with the local op. We transform the local op based on the overlapping remote ops and
    // advance to the next local op.
    let mut transformed: Vec<ChangeOp> = Vec::new();
    let mut local_offset: i64 = 0;
    let mut remote_offset: i64 = 0;
    let mut r = 0;
    for local_change_op in local_change_set.ops.iter() {
        let local_op = local_change_op
            .op
            .as_ref()
            .ok_or_else(|| unexpected_empty_op_error("local"))?;

        match local_op {
            Op::Insert(local_insert) => {
                push_op(&mut transformed, Op::Insert(local_insert.clone()))?;
                continue;
            }
            Op::Retain(local_retain) => {
                let mut transformed_retain_count = local_retain.count;
                let (local_op_start, local_op_end) =
                    (local_offset, local_offset + local_retain.count);
                while r < remote_change_set.ops.len() {
                    let remote_change_op = &remote_change_set.ops[r];
                    let remote_op = remote_change_op
                        .op
                        .as_ref()
                        .ok_or_else(|| unexpected_empty_op_error("remote"))?;
                    match remote_op {
                        Op::Insert(remote_insert) => {
                            transformed_retain_count +=
                                remote_insert.content.chars().count() as i64;
                            r += 1;
                        }
                        Op::Retain(remote_retain) => {
                            let remote_op_end = remote_offset + remote_retain.count;
                            if remote_op_end > local_op_end {
                                break;
                            } else {
                                remote_offset += remote_retain.count;
                                r += 1;
                            }
                        }
                        Op::Delete(remote_delete) => {
                            let (remote_op_start, remote_op_end) =
                                (remote_offset, remote_offset + remote_delete.count);
                            let overlap_len = get_overlap_len(
                                (local_op_start, local_op_end),
                                (remote_op_start, remote_op_end),
                            );
                            transformed_retain_count -= overlap_len;
                            if remote_op_end > local_op_end {
                                break;
                            } else {
                                remote_offset += remote_delete.count;
                                r += 1;
                            }
                        }
                    }
                }
                if transformed_retain_count > 0 {
                    push_op(
                        &mut transformed,
                        Op::Retain(Retain {
                            count: transformed_retain_count,
                        }),
                    )?;
                }
                local_offset += local_retain.count;
            }
            Op::Delete(local_delete) => {
                let mut remote_retained_overlap = 0;
                let (local_op_start, local_op_end) =
                    (local_offset, local_offset + local_delete.count);
                while r < remote_change_set.ops.len() {
                    let remote_change_op = &remote_change_set.ops[r];
                    let remote_op = remote_change_op
                        .op
                        .as_ref()
                        .ok_or_else(|| unexpected_empty_op_error("remote"))?;
                    match remote_op {
                        Op::Insert(remote_insert) => {
                            if remote_retained_overlap > 0 {
                                push_op(
                                    &mut transformed,
                                    Op::Delete(Delete {
                                        count: remote_retained_overlap,
                                    }),
                                )?;
                            }
                            if !remote_insert.content.is_empty() {
                                push_op(
                                    &mut transformed,
                                    Op::Retain(Retain {
                                        count: remote_insert.content.chars().count() as i64,
                                    }),
                                )?;
                            }
                            remote_retained_overlap = 0;
                            r += 1;
                        }
                        Op::Retain(remote_retain) => {
                            let (remote_op_start, remote_op_end) =
                                (remote_offset, remote_offset + remote_retain.count);
                            let overlap_len = get_overlap_len(
                                (local_op_start, local_op_end),
                                (remote_op_start, remote_op_end),
                            );
                            remote_retained_overlap += overlap_len;
                            if remote_op_end > local_op_end {
                                break;
                            } else {
                                remote_offset += remote_retain.count;
                                r += 1;
                            }
                        }
                        Op::Delete(remote_delete) => {
                            let remote_op_end = remote_offset + remote_delete.count;
                            if remote_op_end > local_op_end {
                                break;
                            } else {
                                remote_offset += remote_delete.count;
                                r += 1;
                            }
                        }
                    }
                }
                if remote_retained_overlap > 0 {
                    push_op(
                        &mut transformed,
                        Op::Delete(Delete {
                            count: remote_retained_overlap,
                        }),
                    )?;
                }
                local_offset += local_delete.count;
            }
        }
    }

    // Any remote ops that remain must be Inserts. These become Retains in the transformed output.
    while r < remote_change_set.ops.len() {
        let remote_change_op = &remote_change_set.ops[r];
        let remote_op = remote_change_op
            .op
            .as_ref()
            .ok_or_else(|| unexpected_empty_op_error("remote"))?;
        if let Op::Insert(remote_insert) = remote_op {
            let char_count = remote_insert.content.chars().count() as i64;
            push_op(&mut transformed, Op::Retain(Retain { count: char_count }))?;
        } else {
            return Err(OtError::PostConditionFailed(String::from(
                "Expected all remaining operations in remote change set to be inserts.",
            )));
        }
        r += 1;
    }

    let transformed_local_change_set = ChangeSet { ops: transformed };

    let (transformed_local_len_before, _) =
        get_input_output_doc_lengths(&transformed_local_change_set)?;
    if transformed_local_len_before != remote_len_after {
        return Err(OtError::PostConditionFailed(format!(
            "The transformed local change set must be based on a document of length {}. Is based \
            on a document of length {}",
            remote_len_after, transformed_local_len_before
        )));
    }

    Ok(transformed_local_change_set)
}

/// Composes change sets `A` and `B` into a new change set `AB`. Applying change set `AB` to a
/// document will have the same effect as applying `A` and then `B` in sequence.
///
/// # Formal definition of composition
///
/// A change set can be thought of as a function `f(x) -> y` that takes a document of length `x` as
/// input and returns a document of length `y` as output.
///
/// Let change set `A` be a function `A(x) -> y` that takes a document of length `x` as input and
/// returns a document of length `y` as output.
///
/// Let change set `B` be a function `B(y) -> z` that takes a document of length `y` as input and
/// returns a document of length `z` as output.
///
/// Then the function returns a new change set `AB(x) -> z` which is equivalent to `B(A(x)) -> z`.
///
/// Note: The input document length of `B` must be equal to the output document length of `A`.
/// Otherwise, it is not possible to compose `A` and `B`.
///
/// # How operations are composed
///
/// Here are all of the ways that operations in A and B can be composed:
///
/// - Each `Delete` operation in `A` is added verbatim to the composed change set. Characters
/// deleted in `A` need to be deleted in `AB`.
///
/// - Each `Retain` operation in `A` is composed with operations in `B` as follows:
///   - If it overlaps with an `Insert` in `B`, the `Insert` in `B` is added to the composed change
///   set. Characters inserted in `B` need to be inserted in `AB`.
///   - If it overlaps with a `Retain` in `B`, a `Retain` with length equal to the overlap is added
///   to the composed change set. Characters retained in both `A` and `B` need to be retained in
///   `AB`.
///   - If it overlaps with a `Delete` in `B`, a `Delete` with length equal to the overlap is added
///   to the composed change set. Characters deleted in `B` need to be deleted in `AB`.
///
/// - Each `Insert` operation in `A` is composed with operations in `B` as follows:
///   - If it overlaps with a `Retain` in `B`, all of the characters in the retained region will
///   remain in the composed insert. Characters retained in `B` need to be retained in `AB`.
///   - If it overlaps with a `Delete` in `B`, all of the characters in the deleted region will
///   elided from the composed insert. Characters deleted in `B` need to be deleted in `AB`.
///   - If it overlaps with an `Insert` in `B`, then the contents of the `Insert` in `B` will be
///   added into the composed insert at the position where the overlap starts. Characters inserted
///   in `B` need to be inserted in `AB`.
///
/// - Any trailing `Insert` operations in `B` that do not overlap with operations in `A` are added
/// verbatim to the composed output. Characters inserted in `B` need to be inserted in `AB`.
///
/// # Errors
///
/// - Returns `OtError::InvalidInput` when:
///   - The input document length of `B` is not equal to the output document length of `A` (i.e. it
///   is not possible to compose `A` and `B`).
///   - A change set contains an empty op.
///   - A change set seems malformed.
///
/// - Returns `OtError::PostConditionFailed` when the composed change set does not have the correct
/// input and output document lengths.
///
pub fn compose(a_change_set: &ChangeSet, b_change_set: &ChangeSet) -> Result<ChangeSet, OtError> {
    let (a_input_len, a_output_len) = get_input_output_doc_lengths(a_change_set)?;
    let (b_input_len, b_output_len) = get_input_output_doc_lengths(b_change_set)?;

    if a_output_len != b_input_len {
        return Err(OtError::InvalidInput(format!(
            "Cannot compose change sets A and B. A.output_len does not equal B.input_len.\
            A.output_len: {}, B.input_len: {}",
            a_output_len, b_input_len
        )));
    }

    let unexpected_empty_op_error =
        |name: &str| OtError::InvalidInput(format!("change set {} contained an empty op", name));

    let unexpected_missing_char =
        || OtError::InvalidInput(String::from("Unexpected missing character in insert"));

    let mut composed: Vec<ChangeOp> = Vec::new();
    let mut a_offset = 0;
    let mut b_offset = 0;
    let mut b = 0;
    for a_change_op in a_change_set.ops.iter() {
        let a_op = a_change_op
            .op
            .as_ref()
            .ok_or_else(|| unexpected_empty_op_error("A"))?;

        match a_op {
            Op::Delete(a_delete) => {
                push_op(&mut composed, Op::Delete(a_delete.clone()))?;
                continue;
            }
            Op::Retain(a_retain) => {
                let (a_op_start, a_op_end) = (a_offset, a_offset + a_retain.count);
                while b < b_change_set.ops.len() {
                    let b_change_op = &b_change_set.ops[b];
                    let b_op = b_change_op
                        .op
                        .as_ref()
                        .ok_or_else(|| unexpected_empty_op_error("B"))?;
                    match b_op {
                        Op::Insert(b_insert) => {
                            push_op(
                                &mut composed,
                                Op::Insert(Insert {
                                    content: b_insert.content.clone(),
                                }),
                            )?;
                            b += 1;
                        }
                        Op::Retain(b_retain) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_retain.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            push_op(&mut composed, Op::Retain(Retain { count: overlap_len }))?;
                            if b_op_end > a_op_end {
                                break;
                            } else {
                                b_offset += b_retain.count;
                                b += 1;
                            }
                        }
                        Op::Delete(b_delete) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_delete.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            push_op(&mut composed, Op::Delete(Delete { count: overlap_len }))?;
                            if b_op_end > a_op_end {
                                break;
                            } else {
                                b_offset += b_delete.count;
                                b += 1;
                            }
                        }
                    }
                }
                a_offset += a_retain.count;
            }
            Op::Insert(a_insert) => {
                let a_insert_chars_count = a_insert.content.chars().count() as i64;
                let (a_op_start, a_op_end) = (a_offset, a_offset + a_insert_chars_count);
                let mut a_insert_chars = a_insert.content.chars();
                let mut new_insert_content = String::new();
                while b < b_change_set.ops.len() {
                    let b_change_op = &b_change_set.ops[b];
                    let b_op = b_change_op
                        .op
                        .as_ref()
                        .ok_or_else(|| unexpected_empty_op_error("B"))?;
                    match b_op {
                        Op::Insert(b_insert) => {
                            new_insert_content.push_str(&b_insert.content);
                            b += 1;
                        }
                        Op::Retain(b_retain) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_retain.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            for _ in 0..overlap_len {
                                let ch =
                                    a_insert_chars.next().ok_or_else(unexpected_missing_char)?;
                                new_insert_content.push(ch);
                            }
                            if b_op_end > a_op_end {
                                break;
                            } else {
                                b_offset += b_retain.count;
                                b += 1;
                            }
                        }
                        Op::Delete(b_delete) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_delete.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            for _ in 0..overlap_len {
                                a_insert_chars.next().ok_or_else(unexpected_missing_char)?;
                            }
                            if b_op_end > a_op_end {
                                break;
                            } else {
                                b_offset += b_delete.count;
                                b += 1;
                            }
                        }
                    }
                }
                if !new_insert_content.is_empty() {
                    push_op(
                        &mut composed,
                        Op::Insert(Insert {
                            content: new_insert_content,
                        }),
                    )?;
                }
                a_offset += a_insert_chars_count;
            }
        }
    }

    // Any remaining operations in B must be Inserts. Add them verbatim to the composed output.
    while b < b_change_set.ops.len() {
        let b_change_op = &b_change_set.ops[b];
        let b_op = b_change_op
            .op
            .as_ref()
            .ok_or_else(|| unexpected_empty_op_error("B"))?;
        if let Op::Insert(_) = b_op {
            push_op(&mut composed, b_op.clone())?;
        } else {
            return Err(OtError::PostConditionFailed(String::from(
                "Expected all remaining operations in change set B to be inserts.",
            )));
        }
        b += 1;
    }

    let composed_change_set = ChangeSet { ops: composed };

    let (composed_input_len, composed_output_len) =
        get_input_output_doc_lengths(&composed_change_set)?;
    if composed_input_len != a_input_len || composed_output_len != b_output_len {
        return Err(OtError::PostConditionFailed(format!(
            "The composed change set must have input_len {} and output_len {}. It had input_len {} \
            and output_len {}.", a_input_len, b_output_len, composed_input_len, composed_output_len
        )));
    }

    Ok(composed_change_set)
}

/// Applies the change set to the document, returning a new document.
///
/// You can think of a change set as a list of commands to send to an imaginary cursor. The cursor
/// starts at the beginning of the document, advances a while, inserts some characters, advances
/// again, deletes some characters, etc. Here are the commands:
/// - `Retain(count)`: Advance the cursor by `count` characters.
/// - `Insert(content)`: Insert the string `content` at the current cursor position.
/// - `Delete(count)`: Delete the next `count` characters after the current cursor position.
///
/// # Errors
///
/// - Returns `OtError::InvalidInput` when: the change set is incompatible with the document (i.e.
/// the change set has a input document length that is different from the document's length).
///
/// - Returns `OtError::PostConditionFailed` when the resulting document does not have the same
/// length as the output document length that the change set should produce.
///
pub fn apply(document: &str, change_set: &ChangeSet) -> Result<String, OtError> {
    let (input_len, output_len) = get_input_output_doc_lengths(change_set)?;
    let doc_len = document.chars().count();
    if input_len as usize != doc_len {
        return Err(OtError::InvalidInput(format!(
            "The change set must be based on a document with length {}, but the document had length {}",
            input_len, doc_len,
        )));
    }
    let unexpected_missing_char = || {
        OtError::InvalidInput(String::from(
            "Unexpected missing character in document. Pre-condition failed",
        ))
    };
    let mut new_document = String::new();
    let mut new_doc_len = 0;
    let mut doc_chars = document.chars();
    for change_op in change_set.ops.iter() {
        let op = change_op
            .op
            .as_ref()
            .ok_or_else(|| OtError::InvalidInput(String::from("Change set had an empty op")))?;
        match op {
            Op::Insert(insert) => {
                new_document.push_str(insert.content.as_str());
                new_doc_len += insert.content.chars().count();
            }
            Op::Delete(delete) => {
                for _ in 0..delete.count {
                    doc_chars.next().ok_or_else(unexpected_missing_char)?;
                }
            }
            Op::Retain(retain) => {
                for _ in 0..retain.count {
                    let ch = doc_chars.next().ok_or_else(unexpected_missing_char)?;
                    new_document.push(ch);
                    new_doc_len += 1;
                }
            }
        }
    }
    if output_len as usize != new_doc_len {
        return Err(OtError::PostConditionFailed(format!(
            "After applying changes, the document should have length {}, but it had length {}",
            output_len, new_doc_len,
        )));
    }
    Ok(new_document)
}

/// Transforms the text selection according to the changes included in the change set.
///
/// A selection describes the current cursor position in the text and how many characters are
/// selected after the cursor.
///
/// If the document changes, we may need to adjust a user's cursor position and what text the user
/// has selected (if any).
///
/// # Transformations
///
/// Here are the ways that a selection may be transformed by a new change set:
///
/// - Insert
///   - If N characters are inserted before the selection starts, the selection must be translated
///     to the right by N characters.
///   - If N characters are inserted within the selection, the selection's size must increase by N.
///
/// - Delete
///   - If N characters are deleted before the selection starts, the selection must be translated
///     to the left by N characters.
///   - If N characters are deleted within the selection, the selection's size must decrease by N.
///
/// - Retain: No effect on the selection.
pub fn transform_selection(
    change_set: &ChangeSet,
    selection: &Selection,
) -> Result<Selection, OtError> {
    let mut change_set_offset = 0;
    let mut new_selection_offset = selection.offset;
    let mut new_selection_count = selection.count;
    let (selection_start, selection_end) = (selection.offset, selection.offset + selection.count);
    for change_op in change_set.ops.iter() {
        if change_set_offset >= selection_end {
            break;
        }
        let op = change_op
            .op
            .as_ref()
            .ok_or_else(|| OtError::InvalidInput(String::from("Unexpected missing op")))?;
        match op {
            Op::Retain(retain) => {
                change_set_offset += retain.count;
                continue;
            }
            Op::Insert(insert) => {
                let insert_chars_count = insert.content.chars().count() as i64;
                if change_set_offset < selection_start {
                    new_selection_offset += insert_chars_count;
                } else {
                    new_selection_count += insert_chars_count;
                }
            }
            Op::Delete(delete) => {
                let (delete_op_start, delete_op_end) =
                    (change_set_offset, change_set_offset + delete.count);
                let overlap_len = get_overlap_len(
                    (selection_start, selection_end),
                    (delete_op_start, delete_op_end),
                );
                if delete_op_start < selection_start {
                    let deleted_count_before =
                        get_overlap_len((0, selection_start), (delete_op_start, delete_op_end));
                    new_selection_offset -= deleted_count_before;
                }
                new_selection_count -= overlap_len;
                change_set_offset += delete.count;
            }
        }
    }
    Ok(Selection {
        offset: new_selection_offset,
        count: new_selection_count,
    })
}

impl ChangeSet {
    /// Push a new operation to the end of the `change_ops` list. If the new operation has the same
    /// type as the last operation in `change_ops`, we can extend the last operation instead.
    ///
    /// # Errors
    ///
    /// - Returns `OtError::InvalidInput` when an empty operation is encountered.
    ///
    pub fn push_op(&mut self, new_op: Op) -> Result<(), OtError> {
        push_op(&mut self.ops, new_op)
    }
}

fn push_op(change_ops: &mut Vec<ChangeOp>, new_op: Op) -> Result<(), OtError> {
    let is_empty = match &new_op {
        Op::Insert(insert) => insert.content.is_empty(),
        Op::Delete(delete) => delete.count == 0,
        Op::Retain(retain) => retain.count == 0,
    };
    if is_empty {
        return Ok(());
    }
    if change_ops.is_empty() {
        change_ops.push(ChangeOp { op: Some(new_op) });
        return Ok(());
    }
    let last_op =
        change_ops.last_mut().unwrap().op.as_mut().ok_or_else(|| {
            OtError::InvalidInput(String::from("change set contained an empty op"))
        })?;
    match (last_op, &new_op) {
        (Op::Insert(last_insert), Op::Insert(new_insert)) => {
            last_insert.content.push_str(&new_insert.content);
        }
        (Op::Delete(last_delete), Op::Delete(new_delete)) => {
            last_delete.count += new_delete.count;
        }
        (Op::Retain(last_retain), Op::Retain(new_retain)) => {
            last_retain.count += new_retain.count;
        }
        _ => {
            change_ops.push(ChangeOp { op: Some(new_op) });
        }
    }
    Ok(())
}

fn get_input_output_doc_lengths(change_set: &ChangeSet) -> Result<(i64, i64), OtError> {
    let mut retained: i64 = 0;
    let mut deleted: i64 = 0;
    let mut inserted: i64 = 0;
    for i in 0..change_set.ops.len() {
        match &change_set.ops[i].op {
            Some(Op::Retain(retain)) => {
                retained += retain.count;
            }
            Some(Op::Insert(insert)) => {
                inserted += insert.content.chars().count() as i64;
            }
            Some(Op::Delete(delete)) => {
                deleted += delete.count;
            }
            None => {
                return Err(OtError::InvalidInput(format!(
                    "Unexpected empty op at index {}",
                    i
                )));
            }
        }
    }
    let before = retained + deleted;
    let after = retained + inserted;
    Ok((before, after))
}

fn get_overlap_len(bounds1: (i64, i64), bounds2: (i64, i64)) -> i64 {
    let left = std::cmp::max(bounds1.0, bounds2.0);
    let right = std::cmp::min(bounds1.1, bounds2.1);
    if right >= left {
        right - left
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writing_proto::{change_op::Op, ChangeOp, Delete, Insert, Retain};

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
    fn test_get_input_output_doc_lengths() {
        let change_set = create_change_set(&["R:3", "I:Hello", "D:2", "R:6"]);
        let result = get_input_output_doc_lengths(&change_set);
        assert!(result.is_ok());
        let (input_len, output_len) = result.unwrap();
        assert_eq!(input_len, 11);
        assert_eq!(output_len, 14);
    }

    #[test]
    fn test_basic_transform() {
        // Base document:
        // "Hello, world!"
        //
        // Remote:
        // "Hello there, world!"
        //
        // Local:
        // "Why, hello, world. Good to see you."
        //
        // Local transformed on remote:
        // "Why, hello there, world. Good to see you."
        //
        let base_document = "Hello, world!";

        let remote_change_set = create_change_set(&["R:5", "I: there", "R:8"]);
        let remote_version = apply(base_document, &remote_change_set);
        assert!(remote_version.is_ok());
        let remote_version = remote_version.unwrap();
        assert_eq!(&remote_version, "Hello there, world!");

        let local_change_set = create_change_set(&[
            "I:Why, ",
            "D:1",
            "I:h",
            "R:11",
            "D:1",
            "I:. Good to see you.",
        ]);
        let local_version = apply(base_document, &local_change_set);
        assert!(local_version.is_ok());
        let local_version = local_version.unwrap();
        assert_eq!(&local_version, "Why, hello, world. Good to see you.");

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let transformed_version = apply(&remote_version, &transformed_local_change_set);
        assert!(transformed_version.is_ok());
        let transformed_version = transformed_version.unwrap();
        assert_eq!(
            &transformed_version,
            "Why, hello there, world. Good to see you."
        );
    }

    #[test]
    fn test_transform_remote_insert_before_local_retain() {
        let remote_change_set = create_change_set(&["I:AAA", "R:10"]);

        let local_change_set = create_change_set(&["R:5", "D:5"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:8", "D:5"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_remote_insert_inside_local_retain() {
        let remote_change_set = create_change_set(&["R:2", "I:AAA", "R:8"]);

        let local_change_set = create_change_set(&["R:5", "D:5"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:8", "D:5"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_remote_insert_after_local_retain() {
        let remote_change_set = create_change_set(&["R:5", "I:AAA", "R:5"]);

        let local_change_set = create_change_set(&["R:5", "D:5"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:8", "D:5"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_remote_insert_inside_local_delete() {
        let remote_change_set = create_change_set(&["R:6", "I:AAA", "R:4"]);

        let local_change_set = create_change_set(&["R:5", "D:5"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:5", "D:1", "R:3", "D:4"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_remote_insert_after_local_delete() {
        let remote_change_set = create_change_set(&["R:10", "I:AAA"]);

        let local_change_set = create_change_set(&["R:5", "D:5"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:5", "D:5", "R:3"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_multiple_consecutive_remote_inserts_in_local_retain() {
        let remote_change_set = create_change_set(&["R:3", "I:AAA", "I:BB", "I:CCCC", "R:7"]);

        let local_change_set = create_change_set(&["R:5", "D:5"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:14", "D:5"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_multiple_consecutive_remote_inserts_in_local_delete() {
        let remote_change_set = create_change_set(&["R:3", "I:AAA", "I:BB", "I:CCCC", "R:7"]);

        let local_change_set = create_change_set(&["D:5", "R:5"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["D:3", "R:9", "D:2", "R:5"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_multiple_consecutive_local_inserts() {
        let remote_change_set = create_change_set(&["R:5", "D:5"]);

        let local_change_set = create_change_set(&["R:2", "I:AAA", "I:BB", "I:CCCC", "R:8"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:2", "I:AAABBCCCC", "R:3"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_incompatible_change_set_base_doc_lengths() {
        let remote_change_set = create_change_set(&["R:5", "D:5"]);

        let local_change_set = create_change_set(&["R:2", "I:AAA", "D:3"]);

        let result = transform(&remote_change_set, &local_change_set);
        match result {
            Err(OtError::InvalidInput(_)) => {}
            _ => {
                panic!("Unexpected result: {:?}", result);
            }
        }
    }

    #[test]
    fn test_transform_multiple_trailing_remote_inserts() {
        let remote_change_set = create_change_set(&["R:10", "I:Hello,", "I: world!"]);

        let local_change_set = create_change_set(&["R:5", "D:5", "I:Greetings!"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["R:5", "D:5", "R:13", "I:Greetings!"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_transform_only_inserts() {
        let remote_change_set = create_change_set(&["I:Hello, ", "I:world!"]);

        let local_change_set = create_change_set(&["I: Good to see you!"]);

        let transformed_local_change_set = transform(&remote_change_set, &local_change_set);
        assert!(transformed_local_change_set.is_ok());
        let transformed_local_change_set = transformed_local_change_set.unwrap();
        let expected = create_change_set(&["I: Good to see you!", "R:13"]);
        assert_eq!(transformed_local_change_set, expected);
    }

    #[test]
    fn test_apply_incompatible_change_set_and_document() {
        // Document has length 9.
        let document = "AAABBCCCC";

        // Change set has input document length of 8. Must be 9.
        let change_set = create_change_set(&["R:2", "I:DDD", "D:6"]);

        let result = apply(document, &change_set);
        match result {
            Err(OtError::InvalidInput(_)) => {}
            _ => {
                panic!("Unexpected result: {:?}", result);
            }
        }
    }

    #[test]
    fn test_compose() {
        // Initial document:
        // "Hello, world!"
        //
        // Change Set A:
        // "Hello, world!" => "Hello there, world!"
        //
        // Change Set B:
        // "Hello there, world!" => "Why, hello there, world! It is nice to see you."
        //
        // Composed change set AB:
        // "Hello, world!" => "Why, hello there, world! It is nice to see you."
        //
        let document = "Hello, world!";
        let change_set_a = create_change_set(&["R:5", "I: there", "R:8"]);
        let document_v2 = apply(document, &change_set_a).unwrap();
        assert_eq!(&document_v2, "Hello there, world!");

        let change_set_b =
            create_change_set(&["I:Why, ", "D:1", "I:h", "R:18", "I: It is nice to see you."]);
        let document_v3 = apply(&document_v2, &change_set_b).unwrap();
        assert_eq!(
            &document_v3,
            "Why, hello there, world! It is nice to see you."
        );

        let composed_change_set = compose(&change_set_a, &change_set_b).unwrap();
        let document_after_compose_change_set = apply(document, &composed_change_set).unwrap();
        assert_eq!(&document_after_compose_change_set, &document_v3);
    }

    #[test]
    fn test_compose_only_deletes_in_change_set_a() {
        let change_set_a = create_change_set(&["D:10"]);
        let change_set_b = create_change_set(&["I:Hello, world!"]);
        let composed_change_set = compose(&change_set_a, &change_set_b).unwrap();
        let expected = create_change_set(&["D:10", "I:Hello, world!"]);
        assert_eq!(composed_change_set, expected);
    }

    #[test]
    fn test_transform_selection_insert_before() {
        let change_set = create_change_set(&["R:5", "I:Hello", "R:5"]);
        let selection = Selection {
            offset: 6,
            count: 2,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 11,
            count: 2,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_insert_inside() {
        let change_set = create_change_set(&["R:5", "I:Hello", "R:5"]);
        let selection = Selection {
            offset: 3,
            count: 3,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 3,
            count: 8,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_insert_after() {
        let change_set = create_change_set(&["R:5", "I:Hello", "R:5"]);
        let selection = Selection {
            offset: 2,
            count: 2,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 2,
            count: 2,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_delete_before() {
        // change set: --xx------
        // selection:  -----sss--
        let change_set = create_change_set(&["R:1", "D:2", "R:7"]);
        let selection = Selection {
            offset: 5,
            count: 3,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 3,
            count: 3,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_delete_entirely_inside() {
        // change set: ---xx-----
        // selection:  --ssssssss
        let change_set = create_change_set(&["R:3", "D:2", "R:5"]);
        let selection = Selection {
            offset: 2,
            count: 8,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 2,
            count: 6,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_delete_overlap_left() {
        // change set:  ---xxx----
        // selection:   ----sss---
        let change_set = create_change_set(&["R:3", "D:3", "R:4"]);

        let selection = Selection {
            offset: 4,
            count: 3,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 3,
            count: 1,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_delete_overlap_right() {
        // change set:  -----xxx--
        // selection:   ----sss---
        let change_set = create_change_set(&["R:5", "D:3", "R:2"]);
        let selection = Selection {
            offset: 4,
            count: 3,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 4,
            count: 1,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_delete_full_overlap() {
        // change set:  --xxxxxx--
        // selection:   ---sss----
        let change_set = create_change_set(&["R:2", "D:6", "R:2"]);
        let selection = Selection {
            offset: 3,
            count: 3,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 2,
            count: 0,
        };
        assert_eq!(new_selection, expected);
    }

    #[test]
    fn test_transform_selection_delete_after() {
        // change set:  -------xx-
        // selection:   ---sss----
        let change_set = create_change_set(&["R:7", "D:2", "R:1"]);
        let selection = Selection {
            offset: 3,
            count: 3,
        };

        let new_selection = transform_selection(&change_set, &selection).unwrap();
        let expected = Selection {
            offset: 3,
            count: 3,
        };
        assert_eq!(new_selection, expected);
    }

    // TODO(cliff): Write exhaustive compose tests
}
