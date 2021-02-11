use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusoto_dynamodb::AttributeValue;

pub fn encode_protobuf_message<M>(message: &M) -> Result<Vec<u8>, prost::EncodeError>
where
    M: prost::Message,
{
    let mut encoded = Vec::new();
    match message.encode(&mut encoded) {
        Ok(_) => Ok(encoded),
        Err(e) => Err(e),
    }
}

pub fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Utility trait that allows us to turn an array of tuples into a hash map in one line of code
/// instead of three:
///
/// ```
/// [("mykey".to_string(), AttributeValue { ... })]
///     .to_attribute_value_map()
/// ```
///
/// instead of
///
/// ```
/// [("mykey".to_string(), AttributeValue { ... })]
///     .iter()
///     .cloned()
///     .collect()
/// ```
///
///
pub trait ToAttributeValueMap {
    fn to_attribute_value_map(&self) -> HashMap<String, AttributeValue>;
}

impl ToAttributeValueMap for [(String, AttributeValue)] {
    fn to_attribute_value_map(&self) -> HashMap<String, AttributeValue> {
        self.iter().cloned().collect()
    }
}
