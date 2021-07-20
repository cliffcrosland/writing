#[cfg(test)]
pub mod schema;

use std::collections::HashMap;

use bytes::Bytes;
use rusoto_dynamodb::AttributeValue;

/// In production and staging, DynamoDB table names have a prefix, namely "staging-" and
/// "production-".
#[cfg(not(test))]
pub fn table_name(base_table_name: &str) -> String {
    format!(
        "{}-{}",
        &crate::config::config().dynamodb_env,
        base_table_name
    )
}

/// In local testing, DynamoDB table names have a prefix that depends on the current test thread,
/// like "test4-". This allows database changes in one test thread to be isolated from database
/// changes in another.
#[cfg(test)]
pub fn table_name(base_table_name: &str) -> String {
    let shard = crate::testing::utils::current_test_thread_dynamodb_shard();
    test_table_name(shard, base_table_name)
}

#[cfg(test)]
pub fn test_table_name(test_shard: i32, base_table_name: &str) -> String {
    format!("test{}-{}", test_shard, base_table_name)
}

/// Shorthand to create `AttributeValue` entry with string type `S`.
pub fn av_s(key: &str, value: &str) -> (String, AttributeValue) {
    (
        String::from(key),
        AttributeValue {
            s: Some(String::from(value)),
            ..Default::default()
        },
    )
}

/// Shorthand to create `AttributeValue` entry with number type `N`.
pub fn av_n<T>(key: &str, number: T) -> (String, AttributeValue)
where
    T: std::string::ToString,
{
    (
        String::from(key),
        AttributeValue {
            n: Some(number.to_string()),
            ..Default::default()
        },
    )
}

/// Shorthand to create `AttributeValue` entry with binary type `B`.
pub fn av_b(key: &str, binary: Bytes) -> (String, AttributeValue) {
    (
        String::from(key),
        AttributeValue {
            b: Some(binary),
            ..Default::default()
        },
    )
}

/// Shorthand. Turn an array of `AttributeValue` entries into a hash map.
///
/// eg.
/// ```
/// let input = GetItemInput {
///     key: av_map(&[
///         av_s("my_hash_key", "id123"),
///         av_s("my_range_key", "id456"),
///     ])
/// }
/// ```
pub fn av_map(arr: &[(String, AttributeValue)]) -> HashMap<String, AttributeValue> {
    arr.iter().cloned().collect()
}

/// Shorthand. Retrieve the `S` string value for a given key in a Dynamo item.
pub fn av_get_s<'a>(item: &'a HashMap<String, AttributeValue>, key: &str) -> Option<&'a str> {
    let attribute_value = item.get(key)?;
    let s_value = attribute_value.s.as_ref()?;
    Some(s_value.as_str())
}

/// Shorthand. Retrieve the `N` value for a given key in a Dynamo item, and parse to a number.
pub fn av_get_n<T>(item: &HashMap<String, AttributeValue>, key: &str) -> Option<T>
where
    T: std::str::FromStr,
{
    let attribute_value = item.get(key)?;
    let n_value = attribute_value.n.as_ref()?;
    let number = n_value.parse::<T>().ok()?;
    Some(number)
}

/// Shorthand. Retrieve the `B` binary value for a given ey in a Dynamo item.
pub fn av_get_b<'a>(item: &'a HashMap<String, AttributeValue>, key: &str) -> Option<&'a Bytes> {
    let attribute_value = item.get(key)?;
    let b_value = attribute_value.b.as_ref()?;
    Some(b_value)
}
