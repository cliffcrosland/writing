use std::cmp::Ordering;
use std::ops::Range;

use serde::Serialize;

use ot::writing_proto::change_op::Op;
use ot::writing_proto::ChangeSet;
use ot::OtError;

#[derive(Clone, Debug)]
pub struct DocumentValue {
    pub chunks: Vec<DocumentValueChunk>,
    chunk_id_counter: DocumentValueChunkId,
}

pub type DocumentValueChunkId = usize;
pub type DocumentValueChunkVersion = usize;

#[derive(Clone, Debug)]
pub struct DocumentValueChunk {
    pub id: DocumentValueChunkId,
    pub version: DocumentValueChunkVersion,
    pub offset: usize,
    pub value: Vec<u16>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DocumentValueChunkMeta {
    pub id: DocumentValueChunkId,
    pub version: DocumentValueChunkVersion,
    pub offset: usize,
}

impl DocumentValue {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            chunk_id_counter: 0,
        }
    }

    pub fn value_len(&self) -> usize {
        match self.chunks.last() {
            None => 0,
            Some(chunk) => chunk.offset + chunk.value.len(),
        }
    }

    pub fn apply(&mut self, change_set: &ChangeSet) -> Result<(), OtError> {
        let (input_len, output_len) = ot::get_input_output_doc_lengths(change_set)?;
        let value_len = self.value_len();
        if input_len != value_len as i64 {
            return Err(OtError::InvalidInput(format!(
                "Cannot apply change set. Document length was {}, but change set input length was {}",
                value_len,
                input_len,
            )));
        }

        let original_chunks_len = self.chunks.len();
        let mut chunks_iter = std::mem::take(&mut self.chunks).into_iter();
        self.chunks.reserve(original_chunks_len);
        let mut ops_iter = change_set.ops.iter();

        let mut temp_op: Option<Op>;

        let mut maybe_chunk = chunks_iter.next();
        let mut maybe_op = ot::next_op(&mut ops_iter)?;

        loop {
            match (maybe_chunk, maybe_op) {
                (None, None) => break,
                (chunk, Some(Op::Insert(insert))) => {
                    let content: Vec<u16> = insert.content.iter().map(|ch| *ch as u16).collect();
                    self.append_content(content);
                    maybe_chunk = chunk;
                    maybe_op = ot::next_op(&mut ops_iter)?;
                }
                (Some(mut chunk), Some(Op::Retain(retain))) => {
                    let chunk_len = chunk.value.len() as i64;
                    let retain_count = retain.count;
                    match chunk_len.cmp(&retain_count) {
                        Ordering::Less => {
                            self.append_chunk(chunk);
                            temp_op = Some(ot::retain_op(retain_count - chunk_len));
                            maybe_chunk = chunks_iter.next();
                            maybe_op = temp_op.as_ref();
                        }
                        Ordering::Greater => {
                            let next_chunk_value = chunk.value.split_off(retain_count as usize);
                            self.append_chunk(chunk);
                            maybe_chunk = Some(DocumentValueChunk {
                                id: self.next_chunk_id(),
                                value: next_chunk_value,
                                version: 0,
                                offset: 0,
                            });
                            maybe_op = ot::next_op(&mut ops_iter)?;
                        }
                        Ordering::Equal => {
                            self.append_chunk(chunk);
                            maybe_chunk = chunks_iter.next();
                            maybe_op = ot::next_op(&mut ops_iter)?;
                        }
                    }
                }
                (Some(mut chunk), Some(Op::Delete(delete))) => {
                    let chunk_len = chunk.value.len() as i64;
                    let delete_count = delete.count;
                    match chunk_len.cmp(&delete_count) {
                        Ordering::Less => {
                            temp_op = Some(ot::delete_op(delete_count - chunk_len));
                            maybe_chunk = chunks_iter.next();
                            maybe_op = temp_op.as_ref();
                        }
                        Ordering::Greater => {
                            chunk.value.drain(0..delete_count as usize);
                            chunk.version += 1;
                            maybe_chunk = Some(chunk);
                            maybe_op = ot::next_op(&mut ops_iter)?;
                        }
                        Ordering::Equal => {
                            maybe_chunk = chunks_iter.next();
                            maybe_op = ot::next_op(&mut ops_iter)?;
                        }
                    }
                }
                (None, _) | (_, None) => {
                    return Err(OtError::InvalidInput(String::from(
                        "Mismatched document value and change set ops",
                    )));
                }
            }
        }
        let new_value_len = self.value_len();
        if output_len != new_value_len as i64 {
            return Err(OtError::PostConditionFailed(format!(
                "After applying changes, the document should have length {}, but it had length {}",
                output_len, new_value_len,
            )));
        }
        Ok(())
    }

    pub fn invert(&self, change_set: &ChangeSet) -> Result<ChangeSet, OtError> {
        let (input_len, _output_len) = ot::get_input_output_doc_lengths(change_set)?;
        let value_len = self.value_len();
        if input_len != value_len as i64 {
            return Err(OtError::InvalidInput(format!(
                "Cannot invert change set. Document length was {}, but change set input length was {}",
                value_len,
                input_len,
            )));
        }
        let mut inverted = ChangeSet::new();
        let mut ops_iter = change_set.ops.iter();
        let mut maybe_op = ot::next_op(&mut ops_iter)?;
        let mut i = 0;
        loop {
            match maybe_op {
                None => break,
                Some(Op::Retain(retain)) => {
                    inverted.retain(retain.count);
                    i += retain.count as usize;
                    maybe_op = ot::next_op(&mut ops_iter)?;
                }
                Some(Op::Insert(insert)) => {
                    inverted.delete(insert.content.len() as i64);
                    maybe_op = ot::next_op(&mut ops_iter)?;
                }
                Some(Op::Delete(delete)) => {
                    let content = self.get_value_in_range(i..(i + delete.count as usize))?;
                    inverted.insert_vec_u16(content);
                    i += delete.count as usize;
                    maybe_op = ot::next_op(&mut ops_iter)?;
                }
            }
        }
        Ok(inverted)
    }

    pub fn get_value_in_range(&self, range: Range<usize>) -> Result<Vec<u16>, OtError> {
        let mut start = range.start;
        let end = range.end;
        if start > end {
            return Err(OtError::InvalidInput(format!(
                "Invalid range: {:?}",
                (start..end)
            )));
        }
        if start == end {
            return Ok(Vec::new());
        }
        if self.chunks.is_empty() {
            return Err(OtError::InvalidInput(String::from(
                "Cannot get value range from empty DocumentValue",
            )));
        }
        if end > self.value_len() {
            return Err(OtError::InvalidInput(format!(
                "Invalid range: {:?}. End is greater than DocumentValue length {}",
                (start..end),
                self.value_len()
            )));
        }
        let first = match self
            .chunks
            .binary_search_by(|chunk| chunk.offset.cmp(&start))
        {
            Ok(index) => index,
            Err(index) => index - 1,
        };
        let last = match self
            .chunks
            .binary_search_by(|chunk| (chunk.offset + chunk.value.len()).cmp(&end))
        {
            Ok(index) => index,
            Err(index) => index,
        };
        let mut value_in_range: Vec<u16> = Vec::with_capacity(range.end - range.start);
        for i in first..=last {
            let chunk = &self.chunks[i];
            let slice_start = start - chunk.offset;
            let slice_end = std::cmp::min(end - chunk.offset, chunk.value.len());
            value_in_range.extend_from_slice(&chunk.value[slice_start..slice_end]);
            start = chunk.offset + chunk.value.len();
        }
        Ok(value_in_range)
    }

    pub fn get_chunk_ids(&self) -> Vec<DocumentValueChunkId> {
        self.chunks.iter().map(|chunk| chunk.id).collect()
    }

    pub fn get_chunk_versions(&self) -> Vec<DocumentValueChunkVersion> {
        self.chunks.iter().map(|chunk| chunk.version).collect()
    }

    pub fn get_chunk(&self, id: DocumentValueChunkId) -> Option<&DocumentValueChunk> {
        for i in 0..self.chunks.len() {
            if self.chunks[i].id == id {
                return Some(&self.chunks[i]);
            }
        }
        None
    }

    fn append_chunk(&mut self, mut chunk: DocumentValueChunk) {
        if self.can_append_to_last_chunk() {
            self.append_content(chunk.value);
        } else {
            chunk.offset = match self.chunks.last() {
                None => 0,
                Some(last_chunk) => last_chunk.offset + last_chunk.value.len(),
            };
            self.chunks.push(chunk);
        }
    }

    fn can_append_to_last_chunk(&self) -> bool {
        match self.chunks.last() {
            None => false,
            Some(chunk) => match &chunk.value[..] {
                [.., last_char] if *last_char == '\n' as u16 => false,
                _ => true,
            },
        }
    }

    fn append_content(&mut self, content: Vec<u16>) {
        // If the content is one line, append it directly without any new allocations.
        let newline_index = content.iter().position(|&ch| ch == '\n' as u16);
        let is_one_line = newline_index.is_none() || newline_index.unwrap() == content.len() - 1;
        if is_one_line {
            self.append_content_line(content);
        } else {
            // Otherwise, split into lines, copying each line into a newly allocated Vec.
            let content_lines = content.split_inclusive(|&ch| ch == '\n' as u16);
            for content_line in content_lines {
                self.append_content_line(content_line.to_vec());
            }
        }
    }

    fn append_content_line(&mut self, mut content_line: Vec<u16>) {
        if self.can_append_to_last_chunk() {
            let mut last_chunk = self.chunks.last_mut().unwrap();
            last_chunk.value.append(&mut content_line);
            last_chunk.version += 1;
        } else {
            self.push_new_chunk(content_line);
        }
    }

    fn push_new_chunk(&mut self, content: Vec<u16>) {
        let id = self.next_chunk_id();
        let offset = match self.chunks.last() {
            None => 0,
            Some(last_chunk) => last_chunk.offset + last_chunk.value.len(),
        };
        self.chunks.push(DocumentValueChunk {
            id,
            offset,
            version: 0,
            value: content,
        });
    }

    fn next_chunk_id(&mut self) -> DocumentValueChunkId {
        self.chunk_id_counter += 1;
        self.chunk_id_counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ot::writing_proto::{change_op::Op, ChangeOp, Delete, Insert, Retain};

    fn create_change_set(ops: &[&str]) -> ChangeSet {
        let change_ops: Vec<ChangeOp> = ops
            .iter()
            .map(|op| {
                let op = if let Some(rest) = op.strip_prefix("I:") {
                    let content: Vec<u32> = rest.encode_utf16().map(u16::into).collect();
                    ot::insert_op(&content)
                } else if let Some(rest) = op.strip_prefix("R:") {
                    ot::retain_op(rest.parse::<i64>().unwrap())
                } else if let Some(rest) = op.strip_prefix("D:") {
                    ot::delete_op(rest.parse::<i64>().unwrap())
                } else {
                    unreachable!()
                };
                ChangeOp { op: Some(op) }
            })
            .collect();
        ChangeSet { ops: change_ops }
    }

    #[allow(dead_code)]
    fn print_chunks(document_value: &DocumentValue) {
        for chunk in document_value.chunks.iter() {
            let mut value = String::new();
            for ch in String::from_utf16(&chunk.value).unwrap().chars() {
                if ch == '\n' {
                    value.push_str("\\n");
                } else {
                    value.push(ch);
                }
            }
            println!(
                "DocumentValueChunk(id: {}, version: {}, value: {}",
                chunk.id, chunk.version, value
            );
        }
    }

    #[test]
    fn test_apply_to_empty_document() {
        let mut document_value = DocumentValue::new();
        let change_set = create_change_set(&["I:Hello, world!"]);
        document_value.apply(&change_set).unwrap();
        assert_eq!(document_value.value_len(), 13);
        assert_eq!(document_value.chunks.len(), 1);
        let chunk = &document_value.chunks[0];
        assert_eq!(chunk.id, 1);
        assert_eq!(chunk.version, 0);
        assert_eq!(chunk.offset, 0);
        assert_eq!(String::from_utf16(&chunk.value).unwrap(), "Hello, world!");
    }

    #[test]
    fn test_apply_insert_newline() {
        let mut document_value = DocumentValue::new();
        document_value
            .apply(&create_change_set(&["I:Hello, world!"]))
            .unwrap();

        // Change "Hello, world!" into "Hello,\n world!"
        let change_set = create_change_set(&["R:6", "I:\n", "R:7"]);
        document_value.apply(&change_set).unwrap();
        assert_eq!(document_value.value_len(), 14);
        assert_eq!(document_value.chunks.len(), 2);
        let first_chunk = &document_value.chunks[0];
        let second_chunk = &document_value.chunks[1];
        // Verify that first chunk's id stayed the same, but its version changed. Implies that we
        // did not make a new allocation for the first chunk.
        assert_eq!(first_chunk.id, 1);
        assert_eq!(first_chunk.version, 1);
        assert_eq!(first_chunk.offset, 0);
        assert_eq!(String::from_utf16(&first_chunk.value).unwrap(), "Hello,\n");
        // Verify that second chunk has a new id and starts with a fresh version.
        assert_eq!(second_chunk.id, 2);
        assert_eq!(second_chunk.version, 0);
        assert_eq!(second_chunk.offset, 7);
        assert_eq!(String::from_utf16(&second_chunk.value).unwrap(), " world!");
    }

    #[test]
    fn test_invert() {
        // Simple: invert a deletion from one chunk.
        let mut document_value = DocumentValue::new();
        document_value
            .apply(&create_change_set(&["I:Hello, there friend!"]))
            .unwrap();
        assert_eq!(document_value.chunks.len(), 1);
        let change_set = create_change_set(&["R:7", "D:5", "I:my", "R:8"]);
        let result = document_value.invert(&change_set);
        assert!(result.is_ok());
        let inverted = result.unwrap();
        assert_eq!(
            inverted,
            create_change_set(&["R:7", "I:there", "D:2", "R:8"])
        );

        // Complex: invert a deletion from multiple chunks.
        let mut document_value = DocumentValue::new();
        document_value
            .apply(&create_change_set(&[
                "I:Hello\nthere my\ngood and delightful\nfriend!",
            ]))
            .unwrap();
        assert_eq!(document_value.chunks.len(), 4);
        let change_set = create_change_set(&["R:12", "D:12", "R:18"]);
        let result = document_value.invert(&change_set);
        let inverted = result.unwrap();
        assert_eq!(
            inverted,
            create_change_set(&["R:12", "I:my\ngood and ", "R:18"])
        );
    }
}
