mod config;
mod db;
mod http;
mod proto;
mod utils;

#[cfg(test)]
mod testing;

use std::sync::Arc;

use actix_web::{App, HttpServer};
use sqlx::postgres::PgPool;

use config::config;

pub struct BackendService {
    pub db_pool: PgPool,
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let backend_service = Arc::new(BackendService {
        db_pool: db::create_pool().await?,
    });

    HttpServer::new(move || {
        App::new()
            .data(backend_service.clone())
            .wrap(http::create_identity_service(
                config().cookie_secret.as_bytes(),
                config().cookie_secure,
            ))
            .service(http::basic::marketing)
            .service(http::basic::app)
            .service(http::basic::log_in)
            .service(http::basic::log_out)
            .service(http::api::pages::create_page)
    })
    .bind(format!("127.0.0.1:{}", &config().http_port))?
    .run()
    .await?;

    Ok(())
}
