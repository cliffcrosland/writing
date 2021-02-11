mod config;
mod dynamodb;
mod http;
mod proto;
mod utils;

#[cfg(test)]
mod testing;

use std::sync::Arc;

use actix_web::{App, HttpServer};
use rusoto_dynamodb::DynamoDbClient;

use config::config;

pub struct BackendService {
    pub dynamodb_client: DynamoDbClient,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let dynamodb_region = &config().dynamodb_region;

    HttpServer::new(move || {
        App::new()
            .data(BackendService {
                dynamodb_client: DynamoDbClient::new(dynamodb_region.clone()),
            })
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
