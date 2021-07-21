use actix_session::Session;
use actix_web::http::header;
use actix_web::{get, web, HttpResponse};

use crate::http;
use crate::BackendService;

#[get("/app")]
pub async fn home(
    session: Session,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let _user = http::get_session_user(&session, &service).await?;
    Ok(HttpResponse::SeeOther()
        .set_header(header::LOCATION, "http://localhost:3000/")
        .finish())
}
