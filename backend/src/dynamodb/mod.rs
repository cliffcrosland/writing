use std::collections::HashMap;

use rusoto_dynamodb::AttributeValue;

#[cfg(test)]
pub mod schema;

/// In production and staging, DynamoDB table names have a prefix, namely "staging-" and
/// "production-".
#[cfg(not(test))]
pub fn dynamodb_table_name(base_table_name: &str) -> String {
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
pub fn dynamodb_table_name(base_table_name: &str) -> String {
    let shard = crate::testing::utils::current_test_thread_dynamodb_shard();
    test_dynamodb_table_name(shard, base_table_name)
}

#[cfg(test)]
pub fn test_dynamodb_table_name(test_shard: i32, base_table_name: &str) -> String {
    format!("test{}-{}", test_shard, base_table_name)
}

/// Shorthand to create `AttributeValue` entry with string type `S`.
pub fn av_s(key: &str, value: &str) -> (String, AttributeValue) {
    (
        key.to_string(),
        AttributeValue {
            s: Some(value.to_string()),
            ..Default::default()
        },
    )
}

/// Shorthand to create `AttributeValue` entry with string type `N`.
#[allow(dead_code)]
pub fn av_n(key: &str, number_str: String) -> (String, AttributeValue) {
    (
        key.to_string(),
        AttributeValue {
            n: Some(number_str),
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
    Some(item.get(key)?.s.as_ref()?.as_str())
}

/// Shorthand. Retrieve the `N` value for a given key in a Dynamo item, and parse as an i32.
pub fn av_get_n_i32(item: &HashMap<String, AttributeValue>, key: &str) -> Option<i32> {
    item.get(key)?.n.as_ref()?.parse::<i32>().ok()
}
