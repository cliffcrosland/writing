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

use std::cmp::Ordering;

use thiserror::Error;

pub use proto::writing as writing_proto;

use writing_proto::{change_op::Op, ChangeOp, ChangeSet, Delete, Insert, Retain, Selection};

/// An operational transformation error.
#[derive(Debug, Error)]
pub enum OtError {
    #[error("Invalid Input: {0}")]
    InvalidInput(String),
    #[error("Post Condition Failed: {0}")]
    PostConditionFailed(String),
}

/// Transforms two concurrent changes `(A, B)` into changes `(A', B')` such that `A * B' == B *
/// A'`.
///
/// This is helpful when we discover new remote changes and need to transform our local changes to
/// take into account the new state of the document.
///
/// Notation: `A * B'` means `compose(A, B')`.
///
/// Any characters,
/// - Inserted by `A` will be inserted by `A'` and retained by `B'`.
/// - Inserted by `B` will be inserted by `B'` and retained by `A'`.
/// - Deleted by both `A` and `B` will not be present in `A'` or `B'`.
/// - Deleted by `A` but retained by `B` will be deleted in `A'`.
/// - Deleted by `B` but retained by `A` will be deleted in `B'`.
/// - Retained by both `A` and `B` will be retained by both `A'` and `B'`.
///
/// # Error
///
/// - Returns `OtError::InvalidInput` when:
///   - The local and remote change sets have different input document lengths.
///   - A change set contains an empty op.
///   - A change set seems malformed.
///
pub fn transform(a: &ChangeSet, b: &ChangeSet) -> Result<(ChangeSet, ChangeSet), OtError> {
    let (a_input_len, _) = get_input_output_doc_lengths(a)?;
    let (b_input_len, _) = get_input_output_doc_lengths(b)?;
    if a_input_len != b_input_len {
        return Err(OtError::InvalidInput(format!(
            "Cannot transform A (input length {}) and B (input length {})",
            a_input_len, b_input_len,
        )));
    }

    let mut a_transform = ChangeSet::new();
    let mut b_transform = ChangeSet::new();

    let mut a_ops_iter = a.ops.iter();
    let mut b_ops_iter = b.ops.iter();

    let mut temp_a_op: Option<Op>;
    let mut temp_b_op: Option<Op>;

    let mut maybe_a_op = next_op(&mut a_ops_iter)?;
    let mut maybe_b_op = next_op(&mut b_ops_iter)?;

    loop {
        match (maybe_a_op, maybe_b_op) {
            (None, None) => break,
            (Some(Op::Insert(insert)), _) => {
                // A' must insert whatever new characters A inserted.
                // B' must retain whatever new characters A inserted (since it follows A).
                a_transform.insert_slice(&insert.content);
                b_transform.retain(insert.content.len() as i64);
                maybe_a_op = next_op(&mut a_ops_iter)?;
            }
            (_, Some(Op::Insert(insert))) => {
                // A' must retain whatever new characters B inserted (since it follows B).
                // B' must insert whatever new characters B inserted.
                a_transform.retain(insert.content.len() as i64);
                b_transform.insert_slice(&insert.content);
                maybe_b_op = next_op(&mut b_ops_iter)?;
            }
            (Some(Op::Retain(a_retain)), Some(Op::Retain(b_retain))) => {
                // If characters are retained in both A and B, they must also be retained in both
                // A' and B'. These characters will remain in the output of both A * B' and B * A'.
                match a_retain.count.cmp(&b_retain.count) {
                    Ordering::Less => {
                        a_transform.retain(a_retain.count);
                        b_transform.retain(a_retain.count);
                        temp_b_op = Some(retain_op(b_retain.count - a_retain.count));
                        maybe_b_op = temp_b_op.as_ref();
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                    }
                    Ordering::Greater => {
                        a_transform.retain(b_retain.count);
                        b_transform.retain(b_retain.count);
                        temp_a_op = Some(retain_op(a_retain.count - b_retain.count));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        a_transform.retain(a_retain.count);
                        b_transform.retain(a_retain.count);
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                }
            }
            (Some(Op::Delete(a_delete)), Some(Op::Delete(b_delete))) => {
                // If characters are deleted in both A and B, they will not be present in the input
                // to A' or B' (since A' follows B, and B' follows A).
                match a_delete.count.cmp(&b_delete.count) {
                    Ordering::Less => {
                        temp_b_op = Some(delete_op(b_delete.count - a_delete.count));
                        maybe_b_op = temp_b_op.as_ref();
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                    }
                    Ordering::Greater => {
                        temp_a_op = Some(delete_op(a_delete.count - b_delete.count));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                }
            }
            (Some(Op::Delete(a_delete)), Some(Op::Retain(b_retain))) => {
                // If characters are deleted in A but not in B, they will need to be deleted in A'
                // (since A' follows B).
                match a_delete.count.cmp(&b_retain.count) {
                    Ordering::Less => {
                        a_transform.delete(a_delete.count);
                        temp_b_op = Some(retain_op(b_retain.count - a_delete.count));
                        maybe_b_op = temp_b_op.as_ref();
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                    }
                    Ordering::Greater => {
                        a_transform.delete(b_retain.count);
                        temp_a_op = Some(delete_op(a_delete.count - b_retain.count));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        a_transform.delete(a_delete.count);
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                }
            }
            (Some(Op::Retain(a_retain)), Some(Op::Delete(b_delete))) => {
                // If characters are deleted in B but not in A, they will need to be deleted in B'
                // (since B' follows A).
                match a_retain.count.cmp(&b_delete.count) {
                    Ordering::Less => {
                        b_transform.delete(a_retain.count);
                        temp_b_op = Some(delete_op(b_delete.count - a_retain.count));
                        maybe_b_op = temp_b_op.as_ref();
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                    }
                    Ordering::Greater => {
                        b_transform.delete(b_delete.count);
                        temp_a_op = Some(retain_op(a_retain.count - b_delete.count));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        b_transform.delete(a_retain.count);
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                }
            }
            (None, _) | (_, None) => {
                return Err(OtError::InvalidInput(
                    "Incompatible change sets".to_string(),
                ));
            }
        }
    }
    Ok((a_transform, b_transform))
}

/// Composes change sets `A` and `B` into a new change set `A * B`.
///
/// Applying `A * B` to a document will have the same effect as applying `A` then `B` in sequence.
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
pub fn compose(a: &ChangeSet, b: &ChangeSet) -> Result<ChangeSet, OtError> {
    let (a_input_len, a_output_len) = get_input_output_doc_lengths(a)?;
    let (b_input_len, b_output_len) = get_input_output_doc_lengths(b)?;

    if a_output_len != b_input_len {
        return Err(OtError::InvalidInput(format!(
            "Cannot compose change sets A (output length {}) and B (input length {})",
            a_output_len, b_input_len
        )));
    }

    let mut composed = ChangeSet::new();

    let mut a_ops_iter = a.ops.iter();
    let mut b_ops_iter = b.ops.iter();

    let mut temp_a_op: Option<Op>;
    let mut temp_b_op: Option<Op>;

    let mut maybe_a_op = next_op(&mut a_ops_iter)?;
    let mut maybe_b_op = next_op(&mut b_ops_iter)?;

    loop {
        match (maybe_a_op.as_ref(), maybe_b_op.as_ref()) {
            (None, None) => break,
            (Some(Op::Delete(a_delete)), _) => {
                // Characters deleted by A are deleted by A * B.
                composed.delete(a_delete.count);
                maybe_a_op = next_op(&mut a_ops_iter)?;
            }
            (_, Some(Op::Insert(b_insert))) => {
                // Characters inserted by B are inserted by A * B.
                composed.insert_slice(&b_insert.content);
                maybe_b_op = next_op(&mut b_ops_iter)?;
            }
            (Some(Op::Retain(a_retain)), Some(Op::Retain(b_retain))) => {
                // Characters retained by both A and B are retained by A * B.
                match a_retain.count.cmp(&b_retain.count) {
                    Ordering::Less => {
                        composed.retain(a_retain.count);
                        temp_b_op = Some(retain_op(b_retain.count - a_retain.count));
                        maybe_b_op = temp_b_op.as_ref();
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                    }
                    Ordering::Greater => {
                        composed.retain(b_retain.count);
                        temp_a_op = Some(retain_op(a_retain.count - b_retain.count));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        composed.retain(a_retain.count);
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?
                    }
                }
            }
            (Some(Op::Insert(a_insert)), Some(Op::Delete(b_delete))) => {
                // Characters inserted by A and deleted by B will not be present in A * B. In other
                // words, they are no-ops and will be skipped.
                //
                // Characters inserted by A and *not* deleted by B will be inserted by A * B.
                let a_insert_content_len = a_insert.content.len() as i64;
                match a_insert_content_len.cmp(&b_delete.count) {
                    Ordering::Less => {
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        temp_b_op = Some(delete_op(b_delete.count - a_insert_content_len));
                        maybe_b_op = temp_b_op.as_ref();
                    }
                    Ordering::Greater => {
                        temp_a_op = Some(insert_op(&a_insert.content[b_delete.count as usize..]));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                }
            }
            (Some(Op::Insert(a_insert)), Some(Op::Retain(b_retain))) => {
                // Characters inserted by A and retained by B will be inserted by A * B.
                let a_insert_content_len = a_insert.content.len() as i64;
                match a_insert_content_len.cmp(&b_retain.count) {
                    Ordering::Less => {
                        composed.insert_slice(&a_insert.content);
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        temp_b_op = Some(retain_op(b_retain.count - a_insert_content_len));
                        maybe_b_op = temp_b_op.as_ref();
                    }
                    Ordering::Greater => {
                        composed.insert_slice(&a_insert.content[0..b_retain.count as usize]);
                        temp_a_op = Some(insert_op(&a_insert.content[b_retain.count as usize..]));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        composed.insert_slice(&a_insert.content);
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                }
            }
            (Some(Op::Retain(a_retain)), Some(Op::Delete(b_delete))) => {
                // Characters retained by A and deleted by B will be deleted by A * B.
                match a_retain.count.cmp(&b_delete.count) {
                    Ordering::Less => {
                        composed.delete(a_retain.count);
                        temp_b_op = Some(delete_op(b_delete.count - a_retain.count));
                        maybe_b_op = temp_b_op.as_ref();
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                    }
                    Ordering::Greater => {
                        composed.delete(b_delete.count);
                        temp_a_op = Some(retain_op(a_retain.count - b_delete.count));
                        maybe_a_op = temp_a_op.as_ref();
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                    Ordering::Equal => {
                        composed.delete(a_retain.count);
                        maybe_a_op = next_op(&mut a_ops_iter)?;
                        maybe_b_op = next_op(&mut b_ops_iter)?;
                    }
                }
            }
            (None, _) | (_, None) => {
                return Err(OtError::InvalidInput(
                    "Incompatible change sets".to_string(),
                ));
            }
        }
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
    let mut first = true;
    for change_set in change_sets {
        if first {
            composed = change_set.clone();
            first = false;
        } else {
            composed = compose(&composed, &change_set)?;
        }
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
    let mut i = 0;
    let mut new_document_u16: Vec<u16> = Vec::with_capacity(document_u16.len());
    let mut new_doc_len = 0;
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
                i += delete.count as usize;
            }
            Op::Retain(retain) => {
                new_document_u16.extend_from_slice(&document_u16[i..(i + retain.count as usize)]);
                i += retain.count as usize;
                new_doc_len += retain.count as usize;
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

/// Applies the change set to the given document, which is comprised of a list of chunks. Returns a
/// new list of chunks.
pub fn apply_chunks(
    document_chunks: Vec<Vec<u16>>,
    change_set: &ChangeSet,
) -> Result<Vec<Vec<u16>>, OtError> {
    let (input_len, output_len) = get_input_output_doc_lengths(change_set)?;
    let doc_len: usize = document_chunks
        .iter()
        .fold(0, |sum, chunk| sum + chunk.len());
    if input_len as usize != doc_len {
        return Err(OtError::InvalidInput(format!(
            "The change set must be based on a document with length {}, but the document had length {}",
            input_len, doc_len,
        )));
    }

    let mut new_document_chunks: Vec<Vec<u16>> = Vec::new();

    let mut chunks_iter = document_chunks.into_iter();
    let mut ops_iter = change_set.ops.iter();

    let mut temp_op: Option<Op>;

    let mut maybe_chunk = chunks_iter.next();
    let mut maybe_op = next_op(&mut ops_iter)?;

    loop {
        match (maybe_chunk, maybe_op) {
            (None, None) => break,
            (chunk, Some(Op::Insert(insert))) => {
                let content: Vec<u16> = insert.content.iter().map(|ch| *ch as u16).collect();
                new_document_chunks.push(content);
                maybe_chunk = chunk;
                maybe_op = next_op(&mut ops_iter)?;
            }
            (Some(mut chunk), Some(Op::Retain(retain))) => {
                let chunk_len = chunk.len();
                let retain_count = retain.count as usize;
                match chunk_len.cmp(&retain_count) {
                    Ordering::Less => {
                        new_document_chunks.push(chunk);
                        temp_op = Some(retain_op(retain_count as i64 - chunk_len as i64));
                        maybe_chunk = chunks_iter.next();
                        maybe_op = temp_op.as_ref();
                    }
                    Ordering::Greater => {
                        maybe_chunk = Some(chunk.split_off(retain_count));
                        new_document_chunks.push(chunk);
                        maybe_op = next_op(&mut ops_iter)?;
                    }
                    Ordering::Equal => {
                        new_document_chunks.push(chunk);
                        maybe_chunk = chunks_iter.next();
                        maybe_op = next_op(&mut ops_iter)?;
                    }
                }
            }
            (Some(mut chunk), Some(Op::Delete(delete))) => {
                let chunk_len = chunk.len();
                let delete_count = delete.count as usize;
                match chunk_len.cmp(&delete_count) {
                    Ordering::Less => {
                        temp_op = Some(delete_op(delete_count as i64 - chunk_len as i64));
                        maybe_chunk = chunks_iter.next();
                        maybe_op = temp_op.as_ref();
                    }
                    Ordering::Greater => {
                        maybe_chunk = Some(chunk.split_off(delete_count));
                        maybe_op = next_op(&mut ops_iter)?;
                    }
                    Ordering::Equal => {
                        maybe_chunk = chunks_iter.next();
                        maybe_op = next_op(&mut ops_iter)?;
                    }
                }
            }
            (None, _) | (_, None) => {
                return Err(OtError::InvalidInput(String::from(
                    "Mismatched document chunks and change set ops",
                )));
            }
        }
    }
    let new_doc_len: usize = new_document_chunks
        .iter()
        .fold(0, |sum, chunk| sum + chunk.len());
    if output_len as usize != new_doc_len {
        return Err(OtError::PostConditionFailed(format!(
            "After applying changes, the document should have length {}, but it had length {}",
            output_len, new_doc_len,
        )));
    }
    Ok(new_document_chunks)
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
    selection: &Selection,
    change_set: &ChangeSet,
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
                if change_set_offset <= selection_start {
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

pub fn next_op<'a>(iter: &mut dyn Iterator<Item = &'a ChangeOp>) -> Result<Option<&'a Op>, OtError> {
    match iter.next() {
        None => Ok(None),
        Some(change_op) => {
            let op = change_op
                .op
                .as_ref()
                .ok_or_else(|| OtError::InvalidInput("Empty op encountered".to_string()))?;
            Ok(Some(op))
        }
    }
}

pub fn retain_op(count: i64) -> Op {
    Op::Retain(Retain { count })
}

pub fn delete_op(count: i64) -> Op {
    Op::Delete(Delete { count })
}

pub fn insert_op(content: &[u32]) -> Op {
    Op::Insert(Insert {
        content: content.to_vec(),
    })
}

pub fn create_empty_inverse(change_set: &ChangeSet) -> ChangeSet {
    let mut ret = ChangeSet::new();
    for change_op in change_set.ops.iter() {
        match change_op.op.as_ref() {
            None => {
                ret.ops.push(ChangeOp { op: None });
            }
            Some(Op::Retain(retain)) => {
                ret.retain(retain.count);
            }
            Some(Op::Insert(insert)) => {
                ret.delete(insert.content.len() as i64);
            }
            Some(Op::Delete(delete)) => {
                let mut empty_content: Vec<u32> = Vec::new();
                for _ in 0..delete.count {
                    empty_content.push(' ' as u32);
                }
                ret.insert_slice(&empty_content);
            }
        }
    }
    ret
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

impl std::fmt::Display for ChangeSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Change Set:")?;
        let result = get_input_output_doc_lengths(&self);
        if let Ok((input_len, output_len)) = result {
            writeln!(f, "input/output lengths: ({}, {})", input_len, output_len)?;
        }
        for change_op in self.ops.iter() {
            match change_op.op.as_ref() {
                None => {
                    writeln!(f, "- EMPTY OP")?;
                }
                Some(Op::Retain(retain)) => {
                    writeln!(f, "- Retain({})", retain.count)?;
                }
                Some(Op::Delete(delete)) => {
                    writeln!(f, "- Delete({})", delete.count)?;
                }
                Some(Op::Insert(insert)) => {
                    let content: Vec<u16> = insert.content.iter().map(|ch| *ch as u16).collect();
                    let content_str = String::from_utf16_lossy(&content);
                    writeln!(f, "- Insert(\"{}\")", &content_str)?;
                }
            }
        }
        Ok(())
    }
}

pub fn get_input_output_doc_lengths(change_set: &ChangeSet) -> Result<(i64, i64), OtError> {
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

    fn string_to_vec_u16(string: &str) -> Vec<u16> {
        string.to_string().chars().map(|ch| ch as u16).collect()
    }

    fn slice_u16_to_string(slice_u16: &[u16]) -> String {
        slice_u16.iter().map(|ch| (*ch as u8) as char).collect()
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

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();

        // Core transform assertion:
        // L * R' == R * L'
        let composed1 = compose(&local_change_set, &transformed_remote).unwrap();
        let composed2 = compose(&remote_change_set, &transformed_local).unwrap();
        assert_eq!(composed1, composed2);

        let expected_version = "Why, hello there, world. Good to see you.";
        let transformed_version = apply(&remote_version, &transformed_local).unwrap();
        assert_eq!(&transformed_version, expected_version);
        let transformed_version = apply(&local_version, &transformed_remote).unwrap();
        assert_eq!(&transformed_version, expected_version);
    }

    #[test]
    fn test_transform_remote_insert_before_local_retain() {
        let local_change_set = create_change_set(&["R:5", "D:5"]);
        let remote_change_set = create_change_set(&["I:AAA", "R:10"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:8", "D:5"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["I:AAA", "R:5"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_remote_insert_inside_local_retain() {
        let local_change_set = create_change_set(&["R:5", "D:5"]);
        let remote_change_set = create_change_set(&["R:2", "I:AAA", "R:8"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:8", "D:5"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:2", "I:AAA", "R:3"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_remote_insert_after_local_retain() {
        let local_change_set = create_change_set(&["R:5", "D:5"]);
        let remote_change_set = create_change_set(&["R:5", "I:AAA", "R:5"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:8", "D:5"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:5", "I:AAA"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_remote_insert_inside_local_delete() {
        let local_change_set = create_change_set(&["R:5", "D:5"]);
        let remote_change_set = create_change_set(&["R:6", "I:AAA", "R:4"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:5", "D:1", "R:3", "D:4"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:5", "I:AAA"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_remote_insert_after_local_delete() {
        let local_change_set = create_change_set(&["R:5", "D:5"]);
        let remote_change_set = create_change_set(&["R:10", "I:AAA"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:5", "D:5", "R:3"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:5", "I:AAA"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_multiple_consecutive_remote_inserts_in_local_retain() {
        let local_change_set = create_change_set(&["R:5", "D:5"]);
        let remote_change_set = create_change_set(&["R:3", "I:AAA", "I:BB", "I:CCCC", "R:7"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:14", "D:5"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:3", "I:AAABBCCCC", "R:2"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_multiple_consecutive_remote_inserts_in_local_delete() {
        let local_change_set = create_change_set(&["D:5", "R:5"]);
        let remote_change_set = create_change_set(&["R:3", "I:AAA", "I:BB", "I:CCCC", "R:7"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["D:3", "R:9", "D:2", "R:5"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["I:AAABBCCCC", "R:5"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_multiple_consecutive_local_inserts() {
        let local_change_set = create_change_set(&["R:2", "I:AAA", "I:BB", "I:CCCC", "R:8"]);
        let remote_change_set = create_change_set(&["R:5", "D:5"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:2", "I:AAABBCCCC", "R:3"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:14", "D:5"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_incompatible_change_set_base_doc_lengths() {
        let local_change_set = create_change_set(&["R:2", "I:AAA", "D:3"]);
        let remote_change_set = create_change_set(&["R:5", "D:5"]);

        let result = transform(&local_change_set, &remote_change_set);
        match result {
            Err(OtError::InvalidInput(_)) => {}
            _ => {
                panic!("Unexpected result: {:?}", result);
            }
        }
    }

    #[test]
    fn test_transform_multiple_trailing_remote_inserts() {
        let local_change_set = create_change_set(&["R:5", "D:5", "I:Greetings!"]);
        let remote_change_set = create_change_set(&["R:10", "I:Hello,", "I: world!"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["R:5", "D:5", "I:Greetings!", "R:13"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:15", "I:Hello, world!"]);
        assert_eq!(transformed_remote, expected_remote);
    }

    #[test]
    fn test_transform_only_inserts() {
        let local_change_set = create_change_set(&["I:Hello, ", "I:world!"]);
        let remote_change_set = create_change_set(&["I: Good to see you!"]);

        let result = transform(&local_change_set, &remote_change_set);
        assert!(result.is_ok());
        let (transformed_local, transformed_remote) = result.unwrap();
        let expected_local = create_change_set(&["I:Hello, world!", "R:17"]);
        assert_eq!(transformed_local, expected_local);
        let expected_remote = create_change_set(&["R:13", "I: Good to see you!"]);
        assert_eq!(transformed_remote, expected_remote);
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
    fn test_apply_chunks() {
        let document = "AAABBCCCC";
        let document_vec: Vec<u16> = string_to_vec_u16(&document);
        let change_set = create_change_set(&["R:2", "D:2", "I:DDD", "R:3", "I:E", "R:2"]);
        let new_document_vec = apply_slice(&document_vec, &change_set).unwrap();
        let new_document = slice_u16_to_string(&new_document_vec);
        let expected_new_document = "AADDDBCCECC";
        assert_eq!(new_document, expected_new_document);

        let document_chunks = vec![
            string_to_vec_u16("AAA"),
            string_to_vec_u16("BB"),
            string_to_vec_u16("CCCC"),
        ];
        let new_document_chunks = apply_chunks(document_chunks, &change_set).unwrap();
        let expected_new_document_chunks = vec![
            string_to_vec_u16("AA"),
            string_to_vec_u16("DDD"),
            string_to_vec_u16("B"),
            string_to_vec_u16("CC"),
            string_to_vec_u16("E"),
            string_to_vec_u16("CC"),
        ];
        assert_eq!(new_document_chunks, expected_new_document_chunks);

        let document_chunks: Vec<Vec<u16>> = Vec::new();
        let change_set = create_change_set(&["I:Hello"]);
        let new_document_chunks = apply_chunks(document_chunks, &change_set).unwrap();
        assert_eq!(new_document_chunks.len(), 1);
        let expected_new_document_chunks = vec![
            string_to_vec_u16("Hello"),
        ];
        assert_eq!(new_document_chunks, expected_new_document_chunks);
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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

        let new_selection = transform_selection(&selection, &change_set).unwrap();
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
}
