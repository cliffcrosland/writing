mod config;
mod documents;
mod dynamodb;
mod http;
mod ids;
mod proto;
mod utils;

#[cfg(test)]
mod testing;

use actix_web::{App, HttpServer};
use rusoto_dynamodb::DynamoDbClient;
use std::sync::Arc;

use config::config;

pub struct BackendService {
    pub dynamodb_client: Arc<DynamoDbClient>,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let dynamodb_client = Arc::new(DynamoDbClient::new(config().dynamodb_region.clone()));

    HttpServer::new(move || {
        App::new()
            .data(BackendService {
                dynamodb_client: dynamodb_client.clone(),
            })
            .wrap(http::create_cookie_session(
                config().cookie_secret.as_bytes(),
                config().cookie_secure,
            ))
            .service(http::api::documents::get_document_revisions)
            .service(http::api::documents::submit_document_change_set)
            .service(http::app::home)
            .service(http::marketing::home)
            .service(http::sessions::log_in)
            .service(http::sessions::log_out)
    })
    .bind(format!("127.0.0.1:{}", &config().http_port))?
    .run()
    .await?;

    Ok(())
}
