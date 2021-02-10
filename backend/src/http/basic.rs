use actix_session::Session;
use actix_web::{get, web, HttpResponse};

use crate::BackendService;

#[get("/")]
pub async fn marketing(_service: web::Data<BackendService>) -> actix_web::Result<HttpResponse> {
    Ok(HttpResponse::Ok().body("<p>HTML web page goes here</p>"))
}

#[get("/app")]
pub async fn app(
    session: Session,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let _user = super::get_session_user(&session, &service).await?;
    Ok(HttpResponse::Ok().body("<p>HTML web page goes here</p>"))
}
