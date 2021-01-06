use std::time::Duration;

use sqlx::postgres::PgPool;

use crate::config::config;

pub async fn create_pool() -> anyhow::Result<PgPool> {
    let postgres_config_str = format!(
        "postgres://app:{}@{}:{}/app",
        &config().postgres_db_password,
        &config().postgres_db_host,
        config().postgres_db_port
    );
    let db_pool = PgPool::builder()
        .max_size(config().postgres_db_pool_max_size)
        .max_lifetime(Some(Duration::from_secs(3600 * 12)))
        .build(&postgres_config_str)
        .await?;
    Ok(db_pool)
}
