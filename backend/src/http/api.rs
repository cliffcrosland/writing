pub mod documents {

    use actix_session::Session;
    use actix_web::{error, post, web, HttpResponse};
    use prost::Message;

    use crate::documents;
    use crate::http;
    use crate::proto::writing::{GetDocumentRevisionsRequest, SubmitDocumentChangeSetRequest};
    use crate::BackendService;

    #[post("/api/documents.get_document_revisions")]
    pub async fn get_document_revisions(
        session: Session,
        request_body: actix_web::web::Bytes,
        service: web::Data<BackendService>,
    ) -> actix_web::Result<HttpResponse> {
        let session_user = http::get_session_user(&session, &service).await?;
        let request = GetDocumentRevisionsRequest::decode(&request_body[..])
            .map_err(|_| error::ErrorBadRequest(""))?;
        let response =
            documents::get_document_revisions(&service.dynamodb_client, &session_user, &request)
                .await?;
        http::create_protobuf_http_response(&response)
    }

    #[post("/api/documents.submit_document_change_set")]
    pub async fn submit_document_change_set(
        session: Session,
        request_body: actix_web::web::Bytes,
        service: web::Data<BackendService>,
    ) -> actix_web::Result<HttpResponse> {
        let session_user = http::get_session_user(&session, &service).await?;
        let request = SubmitDocumentChangeSetRequest::decode(&request_body[..])
            .map_err(|_| error::ErrorBadRequest(""))?;
        let response = documents::submit_document_change_set(
            &service.dynamodb_client,
            &session_user,
            &request,
        )
        .await?;
        http::create_protobuf_http_response(&response)
    }
}
