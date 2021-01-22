mod config;
mod db;
mod http;
mod proto;

use std::sync::Arc;

use actix_identity::{CookieIdentityPolicy, IdentityService};
use actix_web::{web, App, HttpServer};
use sqlx::postgres::PgPool;

use config::config;

pub struct BackendService {
    pub db_pool: PgPool,
}

const COOKIE_IDENTITY_MAX_AGE: i64 = 30 * 86400;

fn create_cookie_identity_policy() -> CookieIdentityPolicy {
    CookieIdentityPolicy::new(config().cookie_secret.as_bytes())
        .name("writing_backend")
        .secure(config().cookie_secure)
        .http_only(true)
        .same_site(cookie::SameSite::Strict)
        .max_age(COOKIE_IDENTITY_MAX_AGE)
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let backend_service = web::Data::new(Arc::new(BackendService {
        db_pool: db::create_pool().await?,
    }));

    HttpServer::new(move || {
        App::new()
            .data(backend_service.clone())
            .wrap(IdentityService::new(create_cookie_identity_policy()))
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
