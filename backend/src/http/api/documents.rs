use std::convert::TryInto;

use actix_session::Session;
use actix_web::{error, post, web, HttpResponse};
use prost::Message;
use rusoto_dynamodb::{DynamoDb, QueryInput};

use crate::documents;
use crate::documents::SharingPermission;
use crate::dynamodb::{av_get_n, av_get_s, av_map, av_s, table_name};
use crate::http;
use crate::http::SessionUser;
use crate::proto::writing::GetDocumentRevisionsRequest;
use crate::BackendService;

#[post("/api/documents.get_document_revisions")]
pub async fn get_document_revisions(
    session: Session,
    request_body: actix_web::web::Bytes,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let user = http::get_session_user(&session, &service).await?;
    let request = GetDocumentRevisionsRequest::decode(&request_body[..])
        .map_err(|_| error::ErrorBadRequest(""))?;

    validate_read_permission(&service, &user, &request.org_id, &request.doc_id).await?;

    let response = documents::get_document_revisions(&service.dynamodb_client, &request)
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error::ErrorInternalServerError("")
        })?;

    http::create_protobuf_http_response(&response)
}

#[post("/api/documents.submit_document_change_set")]
pub async fn submit_document_change_set(
    _session: Session,
    _service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    unimplemented!()
}

async fn validate_read_permission(
    service: &BackendService,
    session_user: &SessionUser,
    org_id: &str,
    doc_id: &str,
) -> actix_web::Result<()> {
    if session_user.org_id.as_str() != org_id {
        return Err(error::ErrorForbidden(""));
    }
    let input = QueryInput {
        table_name: table_name("document"),
        key_condition_expression: Some(String::from("id = :doc_id")),
        filter_expression: Some(String::from("org_id = :org_id")),
        projection_expression: Some(String::from(
            "created_by_user_id, org_level_sharing_permission",
        )),
        expression_attribute_values: Some(av_map(&[
            av_s(":doc_id", doc_id),
            av_s(":org_id", org_id),
        ])),
        ..Default::default()
    };
    let result = service.dynamodb_client.query(input).await;
    let output = match result {
        Ok(output) => output,
        Err(e) => {
            log::error!("{}", e);
            return Err(error::ErrorInternalServerError(""));
        }
    };
    if output.items.is_none() || output.count.is_none() || output.count.unwrap() != 1 {
        return Err(error::ErrorNotFound(""));
    }
    let items = output.items.unwrap();
    let error_not_found = || error::ErrorNotFound("");
    let item = items.first().ok_or_else(error_not_found)?;
    let created_by_user_id = av_get_s(item, "created_by_user_id").ok_or_else(error_not_found)?;
    let org_level_sharing_permission: i32 =
        av_get_n(item, "org_level_sharing_permission").ok_or_else(error_not_found)?;
    if created_by_user_id == session_user.user_id.as_str() {
        return Ok(());
    }
    match org_level_sharing_permission.try_into() {
        Ok(SharingPermission::Read) | Ok(SharingPermission::Write) => Ok(()),
        _ => Err(error::ErrorForbidden("")),
    }
}
