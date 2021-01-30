pub mod api;
pub mod basic;

use actix_identity::{CookieIdentityPolicy, Identity, IdentityService};
use actix_web::{error, HttpResponse};
use sqlx::prelude::PgQueryAs;
use uuid::Uuid;

use crate::utils;
use crate::BackendService;

pub struct SessionUser {
    pub id: Uuid,
    pub org_id: Uuid,
}

const COOKIE_IDENTITY_MAX_AGE: i64 = 30 * 86400;

pub fn create_identity_service(
    cookie_secret: &[u8],
    cookie_secure: bool,
) -> IdentityService<CookieIdentityPolicy> {
    let cookie_identity_policy = CookieIdentityPolicy::new(cookie_secret)
        .name("session")
        .secure(cookie_secure)
        .http_only(true)
        .same_site(cookie::SameSite::Strict)
        .max_age(COOKIE_IDENTITY_MAX_AGE);
    IdentityService::new(cookie_identity_policy)
}

pub async fn get_session_user(
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
    let found_org_id: Option<(Uuid,)> = sqlx::query_as("SELECT org_id FROM users where id = $1")
        .bind(&user_id)
        .fetch_optional(&service.db_pool)
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error::ErrorInternalServerError("")
        })?;
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

pub fn create_protobuf_http_response<M>(message: &M) -> actix_web::Result<HttpResponse>
where
    M: prost::Message,
{
    let encoded =
        utils::encode_protobuf_message(message).map_err(|_| error::ErrorInternalServerError(""))?;

    Ok(HttpResponse::Ok()
        .content_type("application/protobuf")
        .body(encoded))
}
