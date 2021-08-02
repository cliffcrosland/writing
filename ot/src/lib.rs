//! A library providing operational transformation functions. Allows multiple users to collaborate
//! on the same document concurrently without conflict.
//!
//! # Note about Unicode
//!
//! For compatibility with web browsers, all operations in this library apply to UTF-16 code
//! points.
//!
//! For example:
//! - `Retain({ count: 4 })` retains four UTF-16 code points.
//! - `Delete({ count: 7 })` deletes seven UTF-16 code points.
//! - `Selection { offset: 10, count: 3 }` skips ten UTF-16 code points and includes the next
//! three.
//! - `Insert({ content: "foo".encode_utf16().map(u16::into).collect() })` inserts the
//! string "foo", which consists of three UTF-16 code points.
//!
//! We use the `std::str::encode_utf16` and `String::from_utf16_lossy` methods to translate between
//! Rust `str` objects and UTF-16 code point sequences.
//!
//! Using `String::from_utf16_lossy` seems dangerous, but we will not lose data if changes
//! submitted to this library originate from valid web browser UI events. The web browser will not
//! allow UI actions to modify a DOM node's text such that the text becomes invalid UTF-16.

mod proto;

pub use proto::writing as writing_proto;

use writing_proto::{change_op::Op, ChangeOp, ChangeSet, Delete, Insert, Retain, Selection};

/// An operational transformation error.
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
    let (remote_input_len, remote_output_len) = get_input_output_doc_lengths(remote_change_set)?;
    let (local_input_len, _) = get_input_output_doc_lengths(local_change_set)?;

    if remote_input_len != local_input_len {
        return Err(OtError::InvalidInput(format!(
            "The input length of the remote change ({}) was not the same as the input length of the \
            local change ({})",
            remote_input_len, local_input_len
        )));
    }

    let unexpected_empty_op_error =
        |name: &str| OtError::InvalidInput(format!("{} change set contained an empty op", name));

    // For each local op, we advance through the remote ops until we have found all remote ops that
    // overlap with the local op. We transform the local op based on the overlapping remote ops and
    // advance to the next local op.
    let mut transformed = ChangeSet::new();
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
                transformed.insert_slice(&local_insert.content);
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
                            transformed_retain_count += remote_insert.content.len() as i64;
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
                    transformed.retain(transformed_retain_count);
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
                                transformed.delete(remote_retained_overlap);
                            }
                            if !remote_insert.content.is_empty() {
                                transformed.retain(remote_insert.content.len() as i64);
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
                    transformed.delete(remote_retained_overlap);
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
            let char_count = remote_insert.content.len() as i64;
            transformed.retain(char_count);
        } else {
            return Err(OtError::PostConditionFailed(String::from(
                "Expected all remaining operations in remote change set to be inserts.",
            )));
        }
        r += 1;
    }

    let (transformed_local_input_len, _) = get_input_output_doc_lengths(&transformed)?;
    if transformed_local_input_len != remote_output_len {
        return Err(OtError::PostConditionFailed(format!(
            "The transformed local change's input length should be equal to the remote change's \
            output length {}, but it was {}.",
            remote_output_len, transformed_local_input_len
        )));
    }

    Ok(transformed)
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
            "Cannot compose change sets A and B. Output length of A does not equal \
            input length of B. Output length of A: {}, Input length of B: {}",
            a_output_len, b_input_len
        )));
    }

    let unexpected_empty_op_error =
        |name: &str| OtError::InvalidInput(format!("change set {} contained an empty op", name));

    let unexpected_missing_char =
        || OtError::InvalidInput(String::from("Unexpected missing character in insert"));

    let mut composed = ChangeSet::new();
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
                composed.delete(a_delete.count);
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
                            composed.insert_slice(&b_insert.content);
                            b += 1;
                        }
                        Op::Retain(b_retain) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_retain.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            composed.retain(overlap_len);
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
                            composed.delete(overlap_len);
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
                let a_insert_chars_count = a_insert.content.len() as i64;
                let (a_op_start, a_op_end) = (a_offset, a_offset + a_insert_chars_count);
                let mut a_insert_chars = a_insert.content.iter();
                let mut new_insert_content = Vec::new();
                while b < b_change_set.ops.len() {
                    let b_change_op = &b_change_set.ops[b];
                    let b_op = b_change_op
                        .op
                        .as_ref()
                        .ok_or_else(|| unexpected_empty_op_error("B"))?;
                    match b_op {
                        Op::Insert(b_insert) => {
                            new_insert_content.extend(b_insert.content.iter());
                            b += 1;
                        }
                        Op::Retain(b_retain) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_retain.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            for _ in 0..overlap_len {
                                let ch =
                                    a_insert_chars.next().ok_or_else(unexpected_missing_char)?;
                                new_insert_content.push(*ch);
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
                    composed.insert_vec(new_insert_content);
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
        if let Op::Insert(b_op_insert) = b_op {
            composed.insert_slice(&b_op_insert.content);
        } else {
            return Err(OtError::PostConditionFailed(String::from(
                "Expected all remaining operations in change set B to be inserts.",
            )));
        }
        b += 1;
    }

    let (composed_input_len, composed_output_len) = get_input_output_doc_lengths(&composed)?;
    if composed_input_len != a_input_len || composed_output_len != b_output_len {
        return Err(OtError::PostConditionFailed(format!(
            "The composed change set must have input_len {} and output_len {}. It had input_len {} \
            and output_len {}.", a_input_len, b_output_len, composed_input_len, composed_output_len
        )));
    }

    Ok(composed)
}

/// Composes a series of change sets into a single change set.
pub fn compose_iter<'a, I>(change_sets: I) -> Result<ChangeSet, OtError>
where
    I: IntoIterator<Item = &'a ChangeSet>,
{
    let mut composed = ChangeSet::new();
    for change_set in change_sets.into_iter() {
        composed = compose(&composed, &change_set)?;
    }
    Ok(composed)
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
    let document_u16: Vec<u16> = document.encode_utf16().collect();
    apply_slice(&document_u16, change_set)
        .map(|new_document_u16| String::from_utf16_lossy(&new_document_u16))
}

pub fn apply_slice(document_u16: &[u16], change_set: &ChangeSet) -> Result<Vec<u16>, OtError> {
    let (input_len, output_len) = get_input_output_doc_lengths(change_set)?;
    let doc_len = document_u16.len();
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
    let mut new_document_u16: Vec<u16> = Vec::new();
    let mut new_doc_len = 0;
    let mut document_u16_iter = document_u16.iter();
    for change_op in change_set.ops.iter() {
        let op = change_op
            .op
            .as_ref()
            .ok_or_else(|| OtError::InvalidInput(String::from("Change set had an empty op")))?;
        match op {
            Op::Insert(insert) => {
                new_document_u16.extend(insert.content.iter().map(|ch| *ch as u16));
                new_doc_len += insert.content.len();
            }
            Op::Delete(delete) => {
                for _ in 0..delete.count {
                    document_u16_iter
                        .next()
                        .ok_or_else(unexpected_missing_char)?;
                }
            }
            Op::Retain(retain) => {
                for _ in 0..retain.count {
                    let ch = document_u16_iter
                        .next()
                        .ok_or_else(unexpected_missing_char)?;
                    new_document_u16.push(*ch);
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
    Ok(new_document_u16)
}

/// Inverts the given `ChangeSet`, turning `Insert` operations into `Delete` operations, and
/// vice versa. To turn `Delete` operations into `Insert`, we need to original document whose
/// characters were deleted.
///
/// Returns an error if the document's length is incompatible with the change set.
pub fn invert(document: &str, change_set: &ChangeSet) -> Result<ChangeSet, OtError> {
    let document_u16: Vec<u16> = document.encode_utf16().collect();
    invert_slice(&document_u16, change_set)
}

pub fn invert_slice(document_u16: &[u16], change_set: &ChangeSet) -> Result<ChangeSet, OtError> {
    let (input_len, _output_len) = get_input_output_doc_lengths(change_set)?;
    let doc_len = document_u16.len();
    if input_len as usize != doc_len {
        return Err(OtError::InvalidInput(format!(
            "The change set must be based on a document with length {}, but the document had length {}",
            input_len, doc_len,
        )));
    }
    let mut inverted_change_set = ChangeSet::new();
    let mut index: usize = 0;
    for change_op in change_set.ops.iter() {
        let op = change_op
            .op
            .as_ref()
            .ok_or_else(|| OtError::InvalidInput(String::from("Change set had an empty op")))?;
        match op {
            Op::Insert(insert) => {
                inverted_change_set.delete(insert.content.len() as i64);
            }
            Op::Delete(delete) => {
                let delete_count = delete.count as usize;
                let content = &document_u16[index..(index + delete_count)];
                let content: Vec<u32> = content.iter().map(|ch| *ch as u32).collect();
                inverted_change_set.insert_vec(content);
                index += delete_count;
            }
            Op::Retain(retain) => {
                inverted_change_set.retain(retain.count);
                index += retain.count as usize;
            }
        }
    }
    Ok(inverted_change_set)
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
                let insert_chars_count = insert.content.len() as i64;
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

/// Transpose takes two arguments, a pair of changes `(A, inverse(A))`, and a change `B`, returning
/// the pair `(transposed(A), inverse(transposed(A)))`. We can use `inverse(transposed(A))` to undo
/// the changes contributed by `A`.
///
/// The transpose operation helps us undo local changes correctly after some remote changes have
/// occurred.
///
/// # Background
///
/// Given a revision log `[..., A]`, undoing `A` can be accomplished by appending `inverse(A)` to
/// the revision log, giving us: `[..., A, inverse(A)]`. This will undo all changes contributed by
/// `A`.
///
/// However, if we have a revision log `[..., A, B]`, say where `A` is a local change but `B` is a
/// remote change, we cannot simply append `inverse(A)` to the revision log. The document has
/// changed since `A` occurred, so `inverse(A)` may undo changes made by `B`. It may not even be
/// composable with `B`.
///
/// Transpose helps us modify `inverse(A)` so that it is composable with `B` and will only undo
/// changes made by `A`. Roughly speaking, transposing `A` and `B` changes their order such that
/// the revision log `[..., A, B]` is equal to the revision log `[..., transposed(B),
/// transposed(A)]`. If we append `inverse(transposed(A))` to either revision log, we get the same
/// composed document where we only undo the changes contributed by `A`.
///
/// The change `transposed(A)` is defined as follows. All characters:
/// - Deleted in `A` are deleted in `transposed(A)`.
/// - Inserted in `A` and not deleted in `B` are inserted in `transposed(A)`.
/// - Retained in `A` and not deleted in `B` are retained in `transposed(A)`.
/// - Inserted in `B` are retained in `transposed(A)`.
/// - Retained in `B` are retained in `transposed(A)`.
/// - Deleted in `B` are not retained in `transposed(A)`.
///
/// Returns `Err(OtError::InvalidInput)` in these cases:
/// - `(A, inverse(A))` is not an inverse pair.
/// - `A` is not composable with `B`.
/// - `A` or `B` contain an invalid empty operation.
pub fn transpose(
    a_change_set_inverse_pair: &(ChangeSet, ChangeSet),
    b_change_set: &ChangeSet,
) -> Result<(ChangeSet, ChangeSet), OtError> {
    let (a_do, a_undo) = (&a_change_set_inverse_pair.0, &a_change_set_inverse_pair.1);
    if !is_inverse_pair(a_do, a_undo) {
        return Err(OtError::InvalidInput(String::from(
            "The changes in the inverse pair are not inverses of one another.",
        )));
    }
    let (_, a_output_len) = get_input_output_doc_lengths(a_do)?;
    let (b_input_len, _) = get_input_output_doc_lengths(b_change_set)?;
    if a_output_len != b_input_len {
        return Err(OtError::InvalidInput(format!(
            "A's output length {} is not equal to B's input length {}",
            a_output_len, b_input_len
        )));
    }
    let unexpected_empty_op_error =
        |name: &str| OtError::InvalidInput(format!("change set {} contained an empty op", name));
    let mut transposed_a_do = ChangeSet::new();
    let mut transposed_a_undo = ChangeSet::new();
    let mut a_offset = 0;
    let mut b = 0;
    let mut b_offset = 0;
    // For each operation in A, find all overlapping operations in B.
    for a in 0..a_do.ops.len() {
        let a_op = &a_do.ops[a]
            .op
            .as_ref()
            .ok_or_else(|| unexpected_empty_op_error("A"))?;
        match a_op {
            // - Any delete operations in A will be present in transposed(A).
            Op::Delete(a_delete) => {
                transposed_a_do.delete(a_delete.count);
                if let Some(Op::Insert(a_undo_insert)) = a_undo.ops[a].op.as_ref() {
                    transposed_a_undo.insert_slice(&a_undo_insert.content);
                } else {
                    // We already verified that (A, inverse(A)) was a valid inverse pair. This branch
                    // should be unreachable.
                    unreachable!();
                }
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
                        // - Any characters inserted in B will be retained in transposed(A).
                        Op::Insert(b_insert) => {
                            let content_len = b_insert.content.len() as i64;
                            transposed_a_do.retain(content_len);
                            transposed_a_undo.retain(content_len);
                            b += 1;
                        }
                        // - Any characters retained in both A and B will be retained in
                        //   transposed(A).
                        Op::Retain(b_retain) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_retain.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            transposed_a_do.retain(overlap_len);
                            transposed_a_undo.retain(overlap_len);
                            if b_op_end > a_op_end {
                                break;
                            } else {
                                b_offset += b_retain.count;
                                b += 1;
                            }
                        }
                        // - Any characters retained in A but deleted in B will *not* be retained
                        //   in transposed(A).
                        Op::Delete(b_delete) => {
                            let b_op_end = b_offset + b_delete.count;
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
                let (a_op_start, a_op_end) = (a_offset, a_offset + a_insert.content.len() as i64);
                let mut a_insert_offset: i64 = 0;
                while b < b_change_set.ops.len() {
                    let b_change_op = &b_change_set.ops[b];
                    let b_op = b_change_op
                        .op
                        .as_ref()
                        .ok_or_else(|| unexpected_empty_op_error("B"))?;
                    match b_op {
                        // - Any characters inserted in B will be retained in transposed(A).
                        Op::Insert(b_insert) => {
                            let content_len = b_insert.content.len() as i64;
                            transposed_a_do.retain(content_len);
                            transposed_a_undo.retain(content_len);
                            b += 1;
                        }
                        // - Any characters inserted in A and retained in B will be inserted in transposed(A).
                        Op::Retain(b_retain) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_retain.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            if overlap_len > 0 {
                                let range = a_insert_offset as usize
                                    ..(a_insert_offset + overlap_len) as usize;
                                transposed_a_do.insert_slice(&a_insert.content[range]);
                                transposed_a_undo.delete(overlap_len);
                                a_insert_offset += overlap_len;
                            }
                            if b_op_end > a_op_end {
                                break;
                            } else {
                                b_offset += b_retain.count;
                                b += 1;
                            }
                        }
                        // - Any characters inserted in A and deleted in B will *not* be inserted in transposed(A).
                        Op::Delete(b_delete) => {
                            let (b_op_start, b_op_end) = (b_offset, b_offset + b_delete.count);
                            let overlap_len =
                                get_overlap_len((a_op_start, a_op_end), (b_op_start, b_op_end));
                            a_insert_offset += overlap_len; // Skip over this part of A's insertion.
                            if b_op_end > a_op_end {
                                break;
                            } else {
                                b_offset += b_delete.count;
                                b += 1;
                            }
                        }
                    }
                }
                a_offset += a_insert.content.len() as i64;
            }
        }
    }
    // Any operations that remain in B must be Inserts. These become Retains in the transposed output.
    while b < b_change_set.ops.len() {
        let b_change_op = &b_change_set.ops[b];
        let b_op = b_change_op
            .op
            .as_ref()
            .ok_or_else(|| unexpected_empty_op_error("B"))?;
        if let Op::Insert(b_insert) = b_op {
            let char_count = b_insert.content.len() as i64;
            transposed_a_do.retain(char_count);
            transposed_a_undo.retain(char_count);
        } else {
            return Err(OtError::PostConditionFailed(String::from(
                "Expected all remaining operations in B to be inserts.",
            )));
        }
        b += 1;
    }
    Ok((transposed_a_do, transposed_a_undo))
}

/// Returns true if and only if the changes are inverses of one another.
///
/// `A` and `B` are inverses of one another if and only if:
/// 1. They have the same number of operations.
/// 2. When `A[i]` is a Retain, `B[i]` is an identical Retain, and vice-versa.
/// 3. When `A[i]` is an Insert with length `K`, `B[i]` is a Delete with count `K`, and vice-versa.
/// 4. When `A[i]` is a Delete with count `K`, `B[i]` is an Insert with length `K`, and vice-versa.
pub fn is_inverse_pair(a: &ChangeSet, b: &ChangeSet) -> bool {
    if a.ops.len() != b.ops.len() {
        return false;
    }
    for i in 0..a.ops.len() {
        let op_a = &a.ops[i].op;
        let op_b = &b.ops[i].op;
        if op_a.is_none() || op_b.is_none() {
            return false;
        }
        let op_a = op_a.as_ref().unwrap();
        let op_b = op_b.as_ref().unwrap();
        match op_a {
            Op::Retain(retain_a) => match op_b {
                Op::Retain(retain_b) if retain_a.count == retain_b.count => {
                    continue;
                }
                _ => {
                    return false;
                }
            },
            Op::Insert(insert_a) => match op_b {
                Op::Delete(delete_b) if insert_a.content.len() as i64 == delete_b.count => {
                    continue;
                }
                _ => {
                    return false;
                }
            },
            Op::Delete(delete_a) => match op_b {
                Op::Insert(insert_b) if delete_a.count == insert_b.content.len() as i64 => {
                    continue;
                }
                _ => {
                    return false;
                }
            },
        }
    }
    true
}

impl ChangeSet {
    /// Creates an empty change set.
    pub fn new() -> Self {
        Self { ops: vec![] }
    }

    /// Appends a `Retain` operation to the change set. If the last operation was a `Retain`, it
    /// will be extended.
    ///
    /// If `count` is less than or equal to zero, the change set will not be changed.
    pub fn retain(&mut self, count: i64) {
        if count <= 0 {
            return;
        }
        self.push_op(Op::Retain(Retain { count }));
    }

    /// Appends a `Delete` operation to the change set. If the last operation was a `Delete`, it
    /// will be extended.
    ///
    /// If `count` is less than or equal to zero, the change set will not be changed.
    pub fn delete(&mut self, count: i64) {
        if count <= 0 {
            return;
        }
        self.push_op(Op::Delete(Delete { count }));
    }

    /// Appends an `Insert` operation to the change set. If the last operation was an `Insert`, it
    /// will be extended to include the new content.
    pub fn insert(&mut self, content: &str) {
        self.push_op(Op::Insert(Insert {
            content: content.encode_utf16().map(u16::into).collect(),
        }));
    }

    /// Appends an `Insert` operation to the change set. If the last operation was an `Insert`, it
    /// will be extended to include the new content.
    ///
    /// Moves the content `Vec` into the change set.
    ///
    /// Each `u32` element of the `Vec` argument represents a UTF-16 character. We use `u32` here
    /// instead of `u16` because we use Protobufs for our network transmission format, and
    /// Protobufs support `u32` integers but not `u16` integers.
    pub fn insert_vec(&mut self, content: Vec<u32>) {
        self.push_op(Op::Insert(Insert { content }));
    }

    /// Appends an `Insert` operation to the change set. If the last operation was an `Insert`, it
    /// will be extended to include the new content.
    ///
    /// Clones the content slice into the change set.
    ///
    /// Each `u32` element of the slice argument represents a UTF-16 character. We use `u32` here
    /// instead of `u16` because we use Protobufs for our network transmission format, and
    /// Protobufs do not support `u16` integers.
    pub fn insert_slice(&mut self, content: &[u32]) {
        self.push_op(Op::Insert(Insert {
            content: content.to_vec(),
        }));
    }

    /// Pushes a new operation to the end of the change set. If the new operation has the same type
    /// as the last operation, we extend the last operation instead.
    pub fn push_op(&mut self, new_op: Op) {
        let op_is_empty = match &new_op {
            Op::Insert(insert) => insert.content.is_empty(),
            Op::Delete(delete) => delete.count == 0,
            Op::Retain(retain) => retain.count == 0,
        };
        if op_is_empty {
            return;
        }
        // Although highly improbable in practice, a change ops list could theoretically contain
        // degenerate operations that have a missing `op` field. Remove any that exist.
        while !self.ops.is_empty() {
            if self.ops.last().unwrap().op.is_some() {
                break;
            } else {
                self.ops.pop();
            }
        }
        if self.ops.is_empty() {
            self.ops.push(ChangeOp { op: Some(new_op) });
            return;
        }
        let last_op = self.ops.last_mut().unwrap().op.as_mut().unwrap();
        match (last_op, &new_op) {
            (Op::Insert(last_insert), Op::Insert(new_insert)) => {
                last_insert.content.extend(new_insert.content.iter());
            }
            (Op::Delete(last_delete), Op::Delete(new_delete)) => {
                last_delete.count += new_delete.count;
            }
            (Op::Retain(last_retain), Op::Retain(new_retain)) => {
                last_retain.count += new_retain.count;
            }
            _ => {
                self.ops.push(ChangeOp { op: Some(new_op) });
            }
        }
    }

    /// Returns true if and only if the change set is empty.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
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
                inserted += insert.content.len() as i64;
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
                            content: rest.encode_utf16().map(u16::into).collect(),
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
    fn test_compose_iter() {
        let change_sets = vec![
            create_change_set(&["I:hello"]),
            create_change_set(&["R:5", "I:, world!"]),
            create_change_set(&["D:1", "I:H", "R:12"]),
        ];
        let composed = compose_iter(&change_sets).unwrap();
        assert_eq!(composed, create_change_set(&["I:Hello, world!"]));
        let pairs = vec![
            (1, create_change_set(&["I:hello"])),
            (2, create_change_set(&["R:5", "I:, world!"])),
            (3, create_change_set(&["D:1", "I:H", "R:12"])),
        ];
        let pairs_iter = pairs.iter().map(|pair| &pair.1);
        let composed = compose_iter(pairs_iter).unwrap();
        assert_eq!(composed, create_change_set(&["I:Hello, world!"]));
        let change_sets = vec![
            create_change_set(&["I:hello"]),
            create_change_set(&["D:10"]),
        ];
        if let Err(OtError::InvalidInput(_)) = compose_iter(&change_sets) {
            assert!(true);
        } else {
            assert!(false, "Expected invalid input error");
        }
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

    #[test]
    fn test_invert_change_set() {
        let document = "foo bar bash baz";
        let change_set = create_change_set(&["R:8", "D:5", "R:3"]);
        let result = invert(document, &change_set);
        assert!(result.is_ok());
        let inverted_change_set = result.unwrap();
        let expected = create_change_set(&["R:8", "I:bash ", "R:3"]);
        assert_eq!(inverted_change_set, expected);

        let incompatible_document = "foo bar";
        let result = invert(incompatible_document, &change_set);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_inverse_pair() {
        let do_change = create_change_set(&["R:10", "I:Hello", "R:5", "D:6", "R:7"]);
        let undo_change = create_change_set(&["R:10", "D:5", "R:5", "I:amigos", "R:7"]);
        assert!(is_inverse_pair(&do_change, &undo_change));

        let undo_wrong_ops_length = create_change_set(&["R:10"]);
        let undo_insert_mismatch = create_change_set(&["R:10", "D:5", "R:5", "I:amigo", "R:7"]);
        let undo_retain_mismatch = create_change_set(&["R:10", "D:5", "R:5", "I:amigos", "D:7"]);
        let undo_delete_mismatch = create_change_set(&["R:10", "D:3", "R:5", "I:amigos", "R:7"]);
        assert!(!is_inverse_pair(&do_change, &undo_wrong_ops_length));
        assert!(!is_inverse_pair(&do_change, &undo_insert_mismatch));
        assert!(!is_inverse_pair(&do_change, &undo_retain_mismatch));
        assert!(!is_inverse_pair(&do_change, &undo_delete_mismatch));
    }

    #[test]
    fn test_transpose() {
        let a_do = create_change_set(&["R:10", "I:Hello", "R:5", "D:6", "R:7"]);
        let a_undo = create_change_set(&["R:10", "D:5", "R:5", "I:amigos", "R:7"]);
        let a_change_set_inverse_pair = (a_do, a_undo);
        let b_change_set = create_change_set(&["R:10", "D:1", "I:h", "R:5", "D:7", "R:4"]);

        let result = transpose(&a_change_set_inverse_pair, &b_change_set);
        assert!(result.is_ok());
        let (transposed_a_do, transposed_a_undo) = result.unwrap();

        let expected_transposed_a_do = create_change_set(&["R:11", "I:ello", "R:1", "D:6", "R:4"]);
        assert_eq!(transposed_a_do, expected_transposed_a_do);
        let expected_transposed_a_undo =
            create_change_set(&["R:11", "D:4", "R:1", "I:amigos", "R:4"]);
        assert_eq!(transposed_a_undo, expected_transposed_a_undo);

        // TODO(cliff): Add more cases? Maybe test that characters retained in B are always
        // retained in A?
    }
}
