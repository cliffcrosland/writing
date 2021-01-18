mod config;
mod db;
mod proto;

use std::sync::Arc;

use actix_identity::{CookieIdentityPolicy, Identity, IdentityService};
use actix_web::{error, post, web, App, HttpResponse, HttpServer};
use prost::Message;
use sqlx::postgres::PgPool;
use sqlx::prelude::PgQueryAs;
use sqlx::types::chrono;
use uuid::Uuid;

use config::config;
use proto::writing::{CreatePageRequest, CreatePageResponse, LoginRequest, Page};

struct BackendService {
    db_pool: PgPool,
}

struct SessionUser {
    id: Uuid,
    org_id: Uuid,
}

async fn get_session_user(
    id: Identity,
    service: &BackendService,
) -> actix_web::Result<SessionUser> {
    let raw_user_id = match id.identity() {
        Some(raw_user_id) => raw_user_id,
        None => {
            return Err(error::ErrorUnauthorized(""));
        }
    };
    let user_id = match Uuid::parse_str(&raw_user_id) {
        Ok(user_id) => user_id,
        Err(e) => {
            // Invalid UUID in cookie. Delete it.
            log::error!("Invalid UUID in cookie: {}", e);
            id.forget();
            return Err(error::ErrorUnauthorized(""));
        }
    };

    // Fetch the user's org_id
    let found_org_id: Option<(Uuid,)> = sqlx::query_as("SELECT org_id FROM users where id = ?")
        .bind(&user_id)
        .fetch_optional(&service.db_pool)
        .await
        .map_err(|_| error::ErrorInternalServerError(""))?;
    if let Some((org_id,)) = found_org_id {
        Ok(SessionUser {
            id: user_id,
            org_id,
        })
    } else {
        // User with given id does not exist. Delete cookie.
        id.forget();
        Err(error::ErrorUnauthorized(""))
    }
}

#[post("/log_in")]
async fn log_in(
    id: Identity,
    body: web::Bytes,
    service: web::Data<Arc<BackendService>>,
) -> actix_web::Result<HttpResponse> {
    // Parse protobuf request
    let request = LoginRequest::decode(body).map_err(error::ErrorBadRequest)?;

    // Query Postgres to find matching user.
    let result: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, hashed_password FROM users WHERE email = ?")
            .bind(&request.email)
            .fetch_optional(&service.db_pool)
            .await
            .map_err(error::ErrorInternalServerError)?;
    if result.is_none() {
        return Ok(HttpResponse::NotFound().finish());
    }
    let (user_id, hashed_password) = result.unwrap();

    // Check to see if password matches
    let password_matched = bcrypt::verify(&request.password, &hashed_password)
        .map_err(error::ErrorInternalServerError)?;
    if !password_matched {
        return Ok(HttpResponse::NotFound().finish());
    }

    // Store identity in cookie
    id.remember(user_id.to_simple().to_string());

    // Respond to user
    Ok(HttpResponse::Ok().finish())
}

#[post("/log_out")]
async fn log_out(id: Identity) -> actix_web::Result<HttpResponse> {
    id.forget();
    // TODO(cliff): Redirect to home?
    Ok(HttpResponse::Ok().finish())
}

#[post("/api.pages.create_page")]
async fn create_page(
    id: Identity,
    body: web::Bytes,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let user = get_session_user(id, &service).await?;
    let request = CreatePageRequest::decode(body).map_err(error::ErrorBadRequest)?;
    let now = chrono::Utc::now();
    let page_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO pages \
        (org_id, title, created_by_user_id, last_edited_by_user_id, created_at, updated_at) \
        VALUES (?, ?, ?, ?, ?, ?) \
        RETURNING id",
    )
    .bind(&user.org_id)
    .bind(&request.title)
    .bind(&user.id)
    .bind(&user.id)
    .bind(&now)
    .bind(&now)
    .fetch_one(&service.db_pool)
    .await
    .map_err(|_| error::ErrorInternalServerError(""))?;

    let now_micros = now.timestamp_nanos() / 1000;
    let response = CreatePageResponse {
        page: Some(Page {
            id: page_id.0.to_simple().to_string(),
            org_id: user.org_id.to_simple().to_string(),
            title: request.title,
            created_by_user_id: user.id.to_simple().to_string(),
            last_edited_by_user_id: user.id.to_simple().to_string(),
            project_owner_user_id: "".to_string(),
            created_at: now_micros,
            updated_at: now_micros,
        }),
    };

    let mut encoded: Vec<u8> = Vec::new();
    response
        .encode(&mut encoded)
        .map_err(|_| error::ErrorInternalServerError(""))?;

    Ok(HttpResponse::Ok()
        .content_type("application/protobuf")
        .body(encoded))
}

/*
#[post("/api.pages.load_page")]
async fn load_page(id: Identity, service: web::Data<BackendService>) -> actix_web::Result<HttpResponse> {
    let user = get_session_user(id, &service).await?;
    let request = LoadPageRequest::decode(body).map_err(error::ErrorBadRequest)?;

}

#[post("/api.pages.update_page_title")]
async fn update_page_title(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api.pages.insert_page_node")]
async fn insert_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api.pages.update_page_node")]
async fn update_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api.pages.delete_page_node")]
async fn delete_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}
*/

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
            .service(log_in)
            .service(log_out)
            .service(create_page)
    })
    .bind(format!("127.0.0.1:{}", &config().http_port))?
    .run()
    .await?;

    Ok(())
}
