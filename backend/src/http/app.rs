use actix_session::Session;
use actix_web::{get, web, HttpResponse};

use crate::http;
use crate::BackendService;

#[get("/app")]
pub async fn home(
    session: Session,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let _user = http::get_session_user(&session, &service).await?;
    Ok(HttpResponse::Ok().body("<p>App home goes here</p>"))
}
