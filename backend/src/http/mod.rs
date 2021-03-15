pub mod api;
pub mod basic;
pub mod sessions;

use actix_session::{CookieSession, Session};
use actix_web::{error, HttpResponse};
use rusoto_dynamodb::{DynamoDb, GetItemInput};

use crate::dynamodb::{av_get_n, av_map, av_s, table_name};
use crate::ids::Id;
use crate::proto::encode_protobuf_message;
use crate::BackendService;

pub struct SessionUser {
    pub user_id: Id,
    pub org_id: Id,
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
    let org_id = extract_session_cookie_id(session, "org_id");
    let user_id = extract_session_cookie_id(session, "user_id");
    if org_id.is_none() || user_id.is_none() {
        session.purge();
        return Err(error::ErrorUnauthorized(""));
    }
    let org_id = org_id.unwrap();
    let user_id = user_id.unwrap();

    let output = service
        .dynamodb_client
        .get_item(GetItemInput {
            table_name: table_name("organization_users"),
            key: av_map(&[
                av_s("org_id", org_id.as_str()),
                av_s("user_id", user_id.as_str()),
            ]),
            projection_expression: Some("role".to_string()),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error::ErrorInternalServerError("")
        })?;
    if output.item.is_none() {
        session.purge();
        return Err(error::ErrorUnauthorized(""));
    }
    let item = output.item.unwrap();
    let role: i32 = av_get_n(&item, "role").ok_or_else(|| error::ErrorUnauthorized(""))?;
    Ok(SessionUser {
        user_id,
        org_id,
        role,
    })
}

#[allow(dead_code)]
pub fn create_protobuf_http_response<M>(message: &M) -> actix_web::Result<HttpResponse>
where
    M: prost::Message,
{
    let encoded =
        encode_protobuf_message(message).map_err(|_| error::ErrorInternalServerError(""))?;

    Ok(HttpResponse::Ok()
        .content_type("application/protobuf")
        .body(encoded))
}

pub fn extract_session_cookie_id(session: &Session, key: &str) -> Option<Id> {
    match session.get::<String>(key) {
        Ok(Some(value)) => Id::parse(&value),
        _ => None,
    }
}
