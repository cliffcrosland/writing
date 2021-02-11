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
    let num = crate::testing::utils::current_test_thread_dynamodb_shard();
    format!("test{}-{}", num, base_table_name)
}
