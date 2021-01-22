use actix_identity::Identity;
use actix_web::error;
use sqlx::prelude::PgQueryAs;
use uuid::Uuid;

use crate::BackendService;

pub struct SessionUser {
    pub id: Uuid,
    pub org_id: Uuid,
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
