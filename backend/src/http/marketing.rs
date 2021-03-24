use actix_web::{get, HttpResponse};

#[get("/")]
pub async fn home() -> actix_web::Result<HttpResponse> {
    Ok(HttpResponse::Ok().body("<p>Marketing home page goes here</p>"))
}
