mod proto;

use proto::writing::{change_op::Op, ChangeOp, ChangeSet, Delete, Retain};

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
/// A change set can also be thought of as a function `f(x) -> y` that takes a document of length
/// `x` as input and returns a document of length `y` as output.
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
///   - The local and remote change sets have different base document lengths (i.e. We receive
///   arguments `r(x) -> y` and `l(p) -> q` where `x != p`),
///   - A change set contains an empty op.
///   - A change set seems malformed.
///
/// - Returns `OtError::PostConditionFailed. when we create a transformed local change set that has
/// a different base document length than the updated document length of the remote change set.
/// This means that the transformed local change set cannot be applied after the remote change set,
/// which is a problem. (i.e. Given `r(x) -> y` and `l(x) -> z`, we created an invalid transformed
/// local change set `l'(p) -> q` where `y != p`).
///
pub fn transform(
    remote_change_set: &ChangeSet,
    local_change_set: &ChangeSet,
) -> Result<ChangeSet, OtError> {
    let (remote_len_before, remote_len_after) =
        get_document_length_before_and_after(remote_change_set)?;
    let (local_len_before, _) = get_document_length_before_and_after(local_change_set)?;

    if remote_len_before != local_len_before {
        return Err(OtError::InvalidInput(format!(
            "Both the remote change set and the local change sets must be based on a document of \
            the same length. Remote base document length: {}, Local base document length: {}",
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

    // Any remaining remote ops should be Inserts. Make them into one Retain in the transformed
    // local change set.
    let mut remote_inserted = 0;
    while r < remote_change_set.ops.len() {
        let remote_change_op = &remote_change_set.ops[r];
        let remote_op = remote_change_op
            .op
            .as_ref()
            .ok_or_else(|| unexpected_empty_op_error("remote"))?;
        if let Op::Insert(remote_insert) = remote_op {
            remote_inserted += remote_insert.content.chars().count() as i64;
        } else {
            return Err(OtError::InvalidInput(String::from(
                "Impossible? Remote change set had retain and/or delete ops beyond the end of the document",
            )));
        }
        r += 1;
    }
    if remote_inserted > 0 {
        push_op(
            &mut transformed,
            Op::Retain(Retain {
                count: remote_inserted,
            }),
        )?;
    }

    let transformed_local_change_set = ChangeSet { ops: transformed };

    let (transformed_local_len_before, _) =
        get_document_length_before_and_after(&transformed_local_change_set)?;
    if transformed_local_len_before != remote_len_after {
        return Err(OtError::PostConditionFailed(format!(
            "The transformed local change set must be based on a document of length {}. Is based \
            on a document of length {}",
            remote_len_after, transformed_local_len_before
        )));
    }

    Ok(transformed_local_change_set)
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
/// the change set has a base document length that is different from the document's length).
///
/// - Returns `OtError::PostConditionFailed` when the resulting document does not have the same
/// length as the output document length that the change set should produce.
///
pub fn apply(document: &str, change_set: &ChangeSet) -> Result<String, OtError> {
    let (before_len, after_len) = get_document_length_before_and_after(change_set)?;
    let doc_len = document.chars().count();
    if before_len as usize != doc_len {
        return Err(OtError::InvalidInput(format!(
            "The change set must be based on a document with length {}, but the document had length {}",
            before_len, doc_len,
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
    if after_len as usize != new_doc_len {
        return Err(OtError::PostConditionFailed(format!(
            "After applying changes, the document should have length {}, but it had length {}",
            after_len, new_doc_len,
        )));
    }
    Ok(new_document)
}

/// Push a new operation to the end of the `change_ops` list. If the new operation has the same
/// type as the last operation in `change_ops`, we can extend the last operation instead.
///
/// # Errors
///
/// - Returns `OtError::InvalidInput` when an empty operation is encountered.
///
fn push_op(change_ops: &mut Vec<ChangeOp>, new_op: Op) -> Result<(), OtError> {
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

fn get_document_length_before_and_after(change_set: &ChangeSet) -> Result<(i64, i64), OtError> {
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
    std::cmp::min(bounds1.1, bounds2.1) - std::cmp::max(bounds1.0, bounds2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::writing::{change_op::Op, ChangeOp, Delete, Insert, Retain};

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
    fn test_get_document_length_before_and_after() {
        let change_set = create_change_set(&["R:3", "I:Hello", "D:2", "R:6"]);
        let result = get_document_length_before_and_after(&change_set);
        assert!(result.is_ok());
        let (before_len, after_len) = result.unwrap();
        assert_eq!(before_len, 11);
        assert_eq!(after_len, 14);
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
            Err(OtError::InvalidInput(_)) => {},
            _ => {
                panic!("Unexpected result: {:?}", result);
            }
        }
    }

    #[test]
    fn test_apply_incompatible_change_set_and_document() {
        // Document has length 9.
        let document = "AAABBCCCC";

        // Change set has base document length of 8. Must be 9.
        let change_set = create_change_set(&["R:2", "I:DDD", "D:6"]);

        let result = apply(document, &change_set);
        match result {
            Err(OtError::InvalidInput(_)) => {},
            _ => {
                panic!("Unexpected result: {:?}", result);
            }
        }
    }
}
