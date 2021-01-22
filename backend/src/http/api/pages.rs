use actix_identity::Identity;
use actix_web::{error, post, web, HttpResponse};
use prost::Message;
use sqlx::prelude::PgQueryAs;
use sqlx::types::chrono;
use uuid::Uuid;

use crate::BackendService;
use crate::http::utils;
use crate::proto::writing::{CreatePageRequest, CreatePageResponse, Page};

#[post("/api/pages.create_page")]
pub async fn create_page(
    id: Identity,
    body: web::Bytes,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let user = utils::get_session_user(id, &service).await?;
    let request = CreatePageRequest::decode(body).map_err(error::ErrorBadRequest)?;
    let now = chrono::Utc::now();
    let page_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO pages \
        (org_id, title, created_by_user_id, last_edited_by_user_id, created_at, updated_at) \
        VALUES (?, ?, ?, ?, ?, ?) \
        RETURNING id",
    )
    .bind(&user.org_id)
    .bind(&request.title)
    .bind(&user.id)
    .bind(&user.id)
    .bind(&now)
    .bind(&now)
    .fetch_one(&service.db_pool)
    .await
    .map_err(|_| error::ErrorInternalServerError(""))?;

    let now_micros = now.timestamp_nanos() / 1000;
    let response = CreatePageResponse {
        page: Some(Page {
            id: page_id.0.to_simple().to_string(),
            org_id: user.org_id.to_simple().to_string(),
            title: request.title,
            created_by_user_id: user.id.to_simple().to_string(),
            last_edited_by_user_id: user.id.to_simple().to_string(),
            project_owner_user_id: "".to_string(),
            created_at: now_micros,
            updated_at: now_micros,
        }),
    };

    let mut encoded: Vec<u8> = Vec::new();
    response
        .encode(&mut encoded)
        .map_err(|_| error::ErrorInternalServerError(""))?;

    Ok(HttpResponse::Ok()
        .content_type("application/protobuf")
        .body(encoded))
}

/*
#[post("/api/pages.load_page")]
async fn load_page(id: Identity, service: web::Data<BackendService>) -> actix_web::Result<HttpResponse> {
    let user = get_session_user(id, &service).await?;
    let request = LoadPageRequest::decode(body).map_err(error::ErrorBadRequest)?;

}

#[post("/api/pages.update_page_title")]
async fn update_page_title(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api/pages.insert_page_node")]
async fn insert_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api/pages.update_page_node")]
async fn update_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api/pages.delete_page_node")]
async fn delete_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}
*/

