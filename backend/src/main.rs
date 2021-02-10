mod config;
mod db;
mod http;
mod proto;
mod utils;

#[cfg(test)]
mod testing;

use std::sync::Arc;

use actix_web::{App, HttpServer};
use rusoto_dynamodb::DynamoDbClient;
use sqlx::postgres::PgPool;

use config::config;

pub struct BackendService {
    pub db_pool: Arc<PgPool>,
    pub dynamodb_client: DynamoDbClient,
}

impl BackendService {
    fn db_pool(&self) -> &PgPool {
        &*self.db_pool
    }
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let postgres_pool = Arc::new(db::create_pool().await?);

    HttpServer::new(move || {
        // All server threads share a global Postgres connection pool. However, each server thread
        // has its own DynamoDB HTTP client.
        let backend_service = BackendService {
            db_pool: postgres_pool.clone(),
            dynamodb_client: DynamoDbClient::new(config().dynamodb_region.clone()),
        };
        App::new()
            .data(backend_service)
            .wrap(http::create_cookie_session(
                config().cookie_secret.as_bytes(),
                config().cookie_secure,
            ))
            .service(http::basic::marketing)
            .service(http::basic::app)
            .service(http::sessions::log_in)
            .service(http::sessions::log_out)
    })
    .bind(format!("127.0.0.1:{}", &config().http_port))?
    .run()
    .await?;

    Ok(())
}
