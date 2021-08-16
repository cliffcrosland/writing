use std::collections::HashMap;

use js_sys::{ArrayBuffer, Date, Promise, Uint8Array};
use thiserror::Error;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{future_to_promise, JsFuture};
use web_sys::{Request, RequestInit, RequestMode, Response};

use ot::writing_proto::{
    CreateDocumentRequest, CreateDocumentResponse, DocumentSharingPermission, GetDocumentRequest,
    GetDocumentResponse, GetDocumentRevisionsRequest, GetDocumentRevisionsResponse,
    ListMyDocumentsRequest, ListMyDocumentsResponse, SubmitDocumentChangeSetRequest,
    SubmitDocumentChangeSetResponse,
};

#[derive(Debug, Error)]
pub enum BackendApiError {
    #[error("Invalid Input: {0}")]
    InvalidInput(String),
    #[error("Server Error: {0}")]
    ServerError(String),
    #[error("Invalid Response: {0}")]
    InvalidResponse(String),
}

pub struct BackendApi {}

impl BackendApi {
    pub async fn create_document(
        request: &CreateDocumentRequest,
    ) -> Result<CreateDocumentResponse, BackendApiError> {
        let url = "/api/documents.create_document";
        Self::execute_backend_api_request(&url, request).await
    }

    pub async fn get_document(
        request: &GetDocumentRequest,
    ) -> Result<GetDocumentResponse, BackendApiError> {
        let url = "/api/documents.get_document";
        Self::execute_backend_api_request(&url, request).await
    }

    pub async fn get_document_revisions(
        request: &GetDocumentRevisionsRequest,
    ) -> Result<GetDocumentRevisionsResponse, BackendApiError> {
        let url = "/api/documents.get_document_revisions";
        Self::execute_backend_api_request(&url, request).await
    }

    pub async fn list_my_documents(
        request: &ListMyDocumentsRequest,
    ) -> Result<ListMyDocumentsResponse, BackendApiError> {
        let url = "/api/documents.list_my_documents";
        Self::execute_backend_api_request(&url, request).await
    }

    pub async fn submit_document_change_set(
        request: &SubmitDocumentChangeSetRequest,
    ) -> Result<SubmitDocumentChangeSetResponse, BackendApiError> {
        let url = "/api/documents.submit_document_change_set";
        Self::execute_backend_api_request(&url, request).await
    }

    async fn execute_backend_api_request<Req, Res>(
        url: &str,
        request: &Req,
    ) -> Result<Res, BackendApiError>
    where
        Req: prost::Message,
        Res: prost::Message + Default,
    {
        // 1. Create JS Request from protobuf request.
        let mut encoded_request = Vec::new();
        request
            .encode(&mut encoded_request)
            .map_err(|e| BackendApiError::InvalidInput(e.to_string()))?;
        let array = Uint8Array::new_with_length(encoded_request.len() as u32);
        array.copy_from(&encoded_request);
        let mut request_opts = RequestInit::new();
        request_opts.method("POST");
        request_opts.mode(RequestMode::SameOrigin);
        request_opts.body(Some(&array.into()));
        let js_request = Request::new_with_str_and_init(url, &request_opts).map_err(|e| {
            BackendApiError::InvalidInput(format!("Error creating Request: {:?}", e))
        })?;
        let window = web_sys::window()
            .ok_or_else(|| BackendApiError::InvalidInput("window not available".to_string()))?;
        let js_response: Response = JsFuture::from(window.fetch_with_request(&js_request))
            .await
            .map_err(|e| BackendApiError::ServerError(format!("Error executing fetch: {:?}", e)))?
            .dyn_into()
            .map_err(|e| {
                BackendApiError::InvalidResponse(format!("Error converting response: {:?}", e))
            })?;
        if !js_response.ok() {
            return Err(BackendApiError::ServerError(
                "Error: Did not receive OK response status".to_string(),
            ));
        }
        let body_promise = js_response.array_buffer().map_err(|e| {
            BackendApiError::InvalidResponse(format!("Error getting array buffer: {:?}", e))
        })?;
        let body_array_buffer: ArrayBuffer = JsFuture::from(body_promise)
            .await
            .map_err(|e| {
                BackendApiError::InvalidResponse(format!("Error getting array buffer: {:?}", e))
            })?
            .dyn_into()
            .map_err(|e| {
                BackendApiError::InvalidResponse(format!("Error converting array buffer: {:?}", e))
            })?;
        let body_uint8_array = Uint8Array::new(body_array_buffer.as_ref());
        // TODO(cliff): Use unsafe function to get view of bytes instead of copying them?
        let mut body_bytes: Vec<u8> = vec![0; body_uint8_array.length() as usize];
        body_uint8_array.copy_to(&mut body_bytes);
        let response = Res::decode(&body_bytes[..]).map_err(|e| {
            BackendApiError::InvalidResponse(format!("Error decoding response: {:?}", e))
        })?;
        Ok(response)
    }
}

#[wasm_bindgen]
pub struct JsBackendApi {}

#[wasm_bindgen]
impl JsBackendApi {
    #[wasm_bindgen(js_name = createDocument)]
    pub fn create_document(title: String) -> Promise {
        let request = CreateDocumentRequest {
            title,
            org_level_sharing_permission: DocumentSharingPermission::None.into(),
        };
        let future = async move {
            match BackendApi::create_document(&request).await {
                Ok(response) => Ok(JsValue::from_serde(&response).unwrap()),
                Err(e) => {
                    let error_message = format!("Error: {:?}", e);
                    let mut map = HashMap::new();
                    map.insert("error".to_string(), error_message);
                    Err(JsValue::from_serde(&map).unwrap())
                }
            }
        };
        future_to_promise(future)
    }

    #[wasm_bindgen(js_name = getDocument)]
    pub fn get_document(doc_id: String) -> Promise {
        let request = GetDocumentRequest { doc_id };
        let future = async move {
            match BackendApi::get_document(&request).await {
                Ok(response) => Ok(JsValue::from_serde(&response).unwrap()),
                Err(e) => {
                    let error_message = format!("Error: {:?}", e);
                    let mut map = HashMap::new();
                    map.insert("error".to_string(), error_message);
                    Err(JsValue::from_serde(&map).unwrap())
                }
            }
        };
        future_to_promise(future)
    }

    #[wasm_bindgen(js_name = listMyDocuments)]
    pub fn list_my_documents(updated_before_date_time: Date) -> Promise {
        let request = ListMyDocumentsRequest {
            updated_before_date_time: updated_before_date_time.to_iso_string().into(),
        };
        let future = async move {
            match BackendApi::list_my_documents(&request).await {
                Ok(response) => Ok(JsValue::from_serde(&response).unwrap()),
                Err(e) => {
                    let error_message = format!("Error: {:?}", e);
                    let mut map = HashMap::new();
                    map.insert("error".to_string(), error_message);
                    Err(JsValue::from_serde(&map).unwrap())
                }
            }
        };
        future_to_promise(future)
    }
}
