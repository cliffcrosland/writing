use actix_session::Session;
use actix_web::http::header;
use actix_web::{error, get, post, web, HttpResponse};
use serde::{Deserialize, Serialize};
use sqlx::prelude::PgQueryAs;
use std::sync::Arc;
use uuid::Uuid;

use crate::BackendService;

#[derive(Deserialize, Serialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

#[post("/log_in")]
pub async fn log_in(
    session: Session,
    form: web::Form<LoginForm>,
    service: web::Data<Arc<BackendService>>,
) -> actix_web::Result<HttpResponse> {
    // Query Postgres to find matching user.
    let result: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, hashed_password FROM users WHERE email = $1")
            .bind(&form.email)
            .fetch_optional(&service.db_pool)
            .await
            .map_err(|_| error::ErrorInternalServerError(""))?;
    if result.is_none() {
        return Ok(HttpResponse::NotFound().finish());
    }
    let (user_id, hashed_password) = result.unwrap();

    // Check to see if password matches
    let password_matched = bcrypt::verify(&form.password, &hashed_password)
        .map_err(|_| error::ErrorInternalServerError(""))?;
    if !password_matched {
        return Ok(HttpResponse::NotFound().finish());
    }

    // Log in using the most recently selected org. Update the last_login_at timestamp to now.
    let org_id: Option<(Uuid,)> = sqlx::query_as(
        "WITH match AS ( \
             SELECT org_id, user_id FROM organization_users \
             WHERE user_id = $1 \
             ORDER BY last_login_at DESC \
             LIMIT 1 \
         ) \
         UPDATE organization_users updated \
         SET last_login_at = now(), updated_at = now() \
         FROM match \
         WHERE updated.org_id = match.org_id AND \
               updated.user_id = match.user_id \
         RETURNING updated.org_id",
    )
    .bind(&user_id)
    .fetch_optional(&service.db_pool)
    .await
    .map_err(|e| {
        log::error!("{}", e);
        error::ErrorInternalServerError("")
    })?;
    let org_id = match org_id {
        Some((org_id,)) => org_id,
        None => {
            return Err(error::ErrorNotFound(""));
        }
    };

    // Store org_id and user_id in session cookie. A user who belong to multiple orgs may switch
    // their org later, which will update org_id in their session.
    session
        .set("org_id", org_id.to_simple().to_string())
        .map_err(|_| error::ErrorInternalServerError(""))?;
    session
        .set("user_id", user_id.to_simple().to_string())
        .map_err(|_| error::ErrorInternalServerError(""))?;

    // We use "303 See Other" redirect so that refreshing the destination page does not re-submit
    // the log_in form via POST.
    //
    // See: https://developer.mozilla.org/en-US/docs/Web/HTTP/Redirections#temporary_redirections
    Ok(HttpResponse::SeeOther()
        .set_header(header::LOCATION, "/app") // TODO(cliff): Do we need full URL?
        .finish())
}

#[post("/log_out")]
pub async fn log_out(session: Session) -> actix_web::Result<HttpResponse> {
    session.purge();
    // TODO(cliff): Redirect to home?
    Ok(HttpResponse::Ok().finish())
}

#[get("/")]
pub async fn marketing(
    _service: web::Data<Arc<BackendService>>,
) -> actix_web::Result<HttpResponse> {
    Ok(HttpResponse::Ok().body("<p>HTML web page goes here</p>"))
}

#[get("/app")]
pub async fn app(
    session: Session,
    service: web::Data<Arc<BackendService>>,
) -> actix_web::Result<HttpResponse> {
    let _user = super::get_session_user(&session, &service).await?;
    Ok(HttpResponse::Ok().body("<p>HTML web page goes here</p>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use actix_web::http::StatusCode;
    use actix_web::test;
    use actix_web::test::TestRequest;
    use actix_web::App;
    use cookie::Cookie;

    use crate::testing::utils::{
        create_user, decrypt_session_cookie_value, default_backend_service, default_cookie_session,
        TestDbPool,
    };

    #[tokio::test]
    async fn test_login_success() {
        // crate::testing::utils::set_log_level(log::LevelFilter::Debug);
        let pool = TestDbPool::new().await;

        let org_id = Uuid::new_v4();
        let user_id = create_user(&pool.db_pool, &org_id, "jane@smith.com", "Jane Smith").await;

        let password = "KDIo*kJDLJ(1j1;;asdf;1;;1testtesttest";
        let hashed_password = bcrypt::hash(password, 4).unwrap();
        sqlx::query("UPDATE users SET hashed_password = $1 WHERE id = $2")
            .bind(&hashed_password)
            .bind(&user_id)
            .execute(&pool.db_pool)
            .await
            .unwrap();

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service(pool.db_id).await)
                .wrap(default_cookie_session())
                .service(log_in),
        )
        .await;

        let login_form = LoginForm {
            email: "jane@smith.com".to_string(),
            password: password.to_string(),
        };
        let request = TestRequest::post()
            .uri("/log_in")
            .header("content-type", "application/x-www-form-urlencoded")
            .set_form(&login_form)
            .to_request();

        let response = test::call_service(&mut test_app, request).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let cookies: Vec<Cookie> = response.response().cookies().collect();
        assert_eq!(cookies.len(), 1);
        let encrypted_cookie = cookies[0].clone().into_owned();
        let decrypted_cookie_value = decrypt_session_cookie_value(&encrypted_cookie, "session");
        assert!(decrypted_cookie_value.is_some());
        let decrypted_cookie_value = decrypted_cookie_value.unwrap();

        let session_map: HashMap<String, String> =
            serde_json::from_str(&decrypted_cookie_value).unwrap();
        let session_user_id: String =
            serde_json::from_str(&session_map.get("user_id").unwrap()).unwrap();
        let session_org_id: String =
            serde_json::from_str(&session_map.get("org_id").unwrap()).unwrap();

        assert_eq!(user_id.to_simple().to_string(), session_user_id);
        assert_eq!(org_id.to_simple().to_string(), session_org_id);
    }

    #[tokio::test]
    async fn test_login_user_not_found() {}

    #[tokio::test]
    async fn test_login_organization_user_not_found() {}
}
