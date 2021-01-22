use std::sync::Arc;
use actix_identity::Identity;
use actix_web::{error, get, post, web, HttpResponse};
use actix_web::http::header;
use serde::Deserialize;
use sqlx::prelude::PgQueryAs;
use uuid::Uuid;

use crate::BackendService;
use super::utils;

#[derive(Deserialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

#[post("/log_in")]
pub async fn log_in(
    id: Identity,
    form: web::Form<LoginForm>,
    service: web::Data<Arc<BackendService>>,
) -> actix_web::Result<HttpResponse> {
    // Query Postgres to find matching user.
    let result: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, hashed_password FROM users WHERE email = ?")
            .bind(&form.email)
            .fetch_optional(&service.db_pool)
            .await
            .map_err(error::ErrorInternalServerError)?;
    if result.is_none() {
        return Ok(HttpResponse::NotFound().finish());
    }
    let (user_id, hashed_password) = result.unwrap();

    // Check to see if password matches
    let password_matched = bcrypt::verify(&form.password, &hashed_password)
        .map_err(error::ErrorInternalServerError)?;
    if !password_matched {
        return Ok(HttpResponse::NotFound().finish());
    }

    // Store identity in cookie
    id.remember(user_id.to_simple().to_string());

    // We use "303 See Other" redirect so that refreshing the destination page does not cause us to
    // re-submit the log_in form via POST.
    //
    // See: https://developer.mozilla.org/en-US/docs/Web/HTTP/Redirections#temporary_redirections
    Ok(HttpResponse::SeeOther()
        .set_header(header::LOCATION, "/app")
        .finish())
}

#[post("/log_out")]
pub async fn log_out(id: Identity) -> actix_web::Result<HttpResponse> {
    id.forget();
    // TODO(cliff): Redirect to home?
    Ok(HttpResponse::Ok().finish())
}

#[get("/")]
pub async fn marketing(
    _service: web::Data<Arc<BackendService>>
) -> actix_web::Result<HttpResponse> {
    Ok(HttpResponse::Ok().body("<p>HTML web page goes here</p>"))
}

#[get("/app")]
pub async fn app(
    id: Identity,
    service: web::Data<Arc<BackendService>>
) -> actix_web::Result<HttpResponse> {
    let _user = utils::get_session_user(id, &service).await?;
    Ok(HttpResponse::Ok().body("<p>HTML web page goes here</p>"))
}
