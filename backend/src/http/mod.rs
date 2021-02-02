pub mod api;
pub mod basic;

use actix_session::{CookieSession, Session};
use actix_web::{error, HttpResponse};
use sqlx::prelude::PgQueryAs;
use uuid::Uuid;

use crate::utils;
use crate::BackendService;

pub struct SessionUser {
    pub id: Uuid,
    pub org_id: Uuid,
    pub role: i32, // TODO(cliff): protobuf enum
}

const SESSION_COOKIE_MAX_AGE: i64 = 30 * 86400; // 30 days

pub fn create_cookie_session(cookie_secret: &[u8], cookie_secure: bool) -> CookieSession {
    CookieSession::private(cookie_secret)
        .name("session")
        .secure(cookie_secure)
        .http_only(true)
        .same_site(cookie::SameSite::Strict)
        .max_age(SESSION_COOKIE_MAX_AGE)
}

pub async fn get_session_user(
    session: &Session,
    service: &BackendService,
) -> actix_web::Result<SessionUser> {
    let org_id = extract_session_cookie_uuid(session, "org_id");
    let user_id = extract_session_cookie_uuid(session, "user_id");
    if org_id.is_none() || user_id.is_none() {
        session.purge();
        return Err(error::ErrorUnauthorized(""));
    }
    let org_id = org_id.unwrap();
    let user_id = user_id.unwrap();

    let role: Option<(i32,)> = sqlx::query_as(
        "SELECT role FROM organization_users \
         WHERE org_id = $1 AND user_id = $2 \
         LIMIT 1",
    )
    .bind(&org_id)
    .bind(&user_id)
    .fetch_optional(&service.db_pool)
    .await
    .map_err(|e| {
        log::error!("{}", e);
        error::ErrorInternalServerError("")
    })?;
    let role = match role {
        Some((role,)) => role,
        None => {
            session.purge();
            return Err(error::ErrorUnauthorized(""));
        }
    };
    Ok(SessionUser {
        role,
        org_id,
        id: user_id,
    })
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

pub fn extract_session_cookie_uuid(session: &Session, key: &str) -> Option<Uuid> {
    let value = match session.get::<String>(key) {
        Ok(Some(value)) => value,
        _ => {
            return None;
        }
    };
    Uuid::parse_str(&value).ok()
}
