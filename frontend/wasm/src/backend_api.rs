use js_sys::{ArrayBuffer, Uint8Array};
use thiserror::Error;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use ot::writing_proto::{
    GetDocumentRevisionsRequest, GetDocumentRevisionsResponse, SubmitDocumentChangeSetRequest,
    SubmitDocumentChangeSetResponse,
};

const API_HOST_URL: &str = "http://localhost:8000";

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
    pub async fn get_document_revisions(
        request: &GetDocumentRevisionsRequest,
    ) -> Result<GetDocumentRevisionsResponse, BackendApiError> {
        let url = format!("{}/api/documents.get_document_revisions", API_HOST_URL,);
        Self::execute_backend_api_request(&url, request).await
    }

    pub async fn submit_document_change_set(
        request: &SubmitDocumentChangeSetRequest,
    ) -> Result<SubmitDocumentChangeSetResponse, BackendApiError> {
        let url = format!("{}/api/documents.submit_document_change_set", API_HOST_URL,);
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
        let mut body_bytes: Vec<u8> = Vec::with_capacity(body_uint8_array.length() as usize);
        // TODO(cliff): Use unsafe function to get view of bytes instead of copying them?
        body_uint8_array.copy_to(&mut body_bytes);
        let response = Res::decode(&body_bytes[..]).map_err(|e| {
            BackendApiError::InvalidResponse(format!("Error decoding response: {:?}", e))
        })?;
        Ok(response)
    }
}
