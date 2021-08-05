use std::ops::Range;
use std::sync::{Arc, Mutex};

use thiserror::Error;

use ot::writing_proto::submit_document_change_set_response;
use ot::writing_proto::{
    ChangeSet, DocumentRevision, GetDocumentRevisionsRequest, SubmitDocumentChangeSetRequest,
};
use ot::OtError;

use crate::backend_api::{BackendApi, BackendApiError};
use crate::document_editor::get_change_set_description;

#[derive(Debug, Error)]
pub enum CommittedLogError {
    #[error("Backend API Error: {0}")]
    BackendApiError(BackendApiError),
    #[error("Ot Error: {0}")]
    OtError(OtError),
    #[error("Invalid Response Error: {0}")]
    InvalidResponseError(String),
    #[error("Invalid State Error: {0}")]
    InvalidStateError(String),
}

pub struct CommittedLog {
    doc_id: String,
    org_id: String,
    inner: Arc<Mutex<CommittedLogInner>>,
}

struct CommittedLogInner {
    // List of all committed revisions.
    revisions: Vec<DocumentRevision>,
}

pub struct ComposedRemoteRevisions {
    pub composed_change_sets: ChangeSet,
    pub revision_range: (i64, i64),
}

impl CommittedLog {
    pub fn new(doc_id: &str, org_id: &str) -> Self {
        Self {
            doc_id: doc_id.to_string(),
            org_id: org_id.to_string(),
            inner: Arc::new(Mutex::new(CommittedLogInner {
                revisions: Vec::new(),
            })),
        }
    }

    pub fn len(&self) -> usize {
        let inner = self.inner.lock().unwrap();
        inner.revisions.len()
    }

    pub fn compose_range(&self, range: Range<usize>) -> Result<Option<ChangeSet>, OtError> {
        let inner = self.inner.lock().unwrap();
        if range.start >= inner.revisions.len() {
            return Ok(None);
        }
        if range.end <= range.start {
            return Ok(None);
        }
        let iter = inner.revisions[range.start..range.end]
            .iter()
            .map(|rev| rev.change_set.as_ref().unwrap());
        Ok(Some(ot::compose_iter(iter)?))
    }

    /// Commits the given local change set, sending it to the server to add to the document
    /// revisions log.
    ///
    /// Returns one of these response codes:
    ///
    /// - Ack: The local revision was successfully committed to the document revisions log on the
    /// server. It has also been appended to the committed log on the client.
    ///
    /// - DiscoveredNewRevisions: We discovered new remote revisions on the server that the client
    /// does not yet know about. The given local revision was not committed.
    pub async fn commit_local_change_set(
        &self,
        change_set: &ChangeSet,
    ) -> Result<submit_document_change_set_response::ResponseCode, CommittedLogError> {
        use submit_document_change_set_response::ResponseCode;
        let mut request = SubmitDocumentChangeSetRequest {
            doc_id: self.doc_id.clone(),
            org_id: self.org_id.clone(),
            change_set: Some(change_set.clone()),
            ..SubmitDocumentChangeSetRequest::default()
        };
        {
            let inner = self.inner.lock().unwrap();
            request.on_revision_number = inner.last_revision_number();
        }
        let inner = self.inner.clone();
        let mut response = BackendApi::submit_document_change_set(&request)
            .await
            .map_err(CommittedLogError::BackendApiError)?;
        match response.response_code() {
            ResponseCode::DiscoveredNewRevisions => {
                // New remote revisions were discovered. Could not commit this local revision.
                Ok(ResponseCode::DiscoveredNewRevisions)
            }
            ResponseCode::Ack => {
                // Successfully committed this local revision.
                if response.revisions.len() != 1 {
                    return Err(CommittedLogError::InvalidResponseError(format!(
                        "Expected response to contain 1 document revision. Contained {}.",
                        response.revisions.len()
                    )));
                }
                let mut inner = inner.lock().unwrap();
                inner.revisions.push(response.revisions.pop().unwrap());
                Ok(ResponseCode::Ack)
            }
            _ => Err(CommittedLogError::InvalidResponseError(String::from(
                "Response status code was neither Ack nor DiscoveredNewRevisions",
            ))),
        }
    }

    /// Loads new remote revisions from the server.
    ///
    /// If there are new remote revisions, loads them all from the server and adds them to the
    /// committed log. Composes the new remote revisions into a single change set and returns them.
    /// We can use the composed remote revisions to transform our local revisions.
    ///
    /// If there are no new remote revisions, returns `None`.
    pub async fn load_new_remote_revisions(
        &self,
    ) -> Result<Option<ComposedRemoteRevisions>, CommittedLogError> {
        let inner = self.inner.clone();
        // Query for new remote revisions that have revision_number greater than the last revision
        // number in our log.
        let mut last_revision_number;
        {
            let inner = inner.lock().unwrap();
            last_revision_number = inner.last_revision_number();
        }

        // Read batches of new remote revisions from the backend API.
        let mut request = GetDocumentRevisionsRequest {
            doc_id: self.doc_id.clone(),
            org_id: self.org_id.clone(),
            ..GetDocumentRevisionsRequest::default()
        };
        let mut composed_change_sets = ChangeSet::new();
        let mut first_revision_number = None;
        loop {
            request.after_revision_number = last_revision_number;
            // 1. Execute API request
            let response = BackendApi::get_document_revisions(&request)
                .await
                .map_err(CommittedLogError::BackendApiError)?;
            if response.revisions.is_empty() {
                break;
            }
            if first_revision_number.is_none() {
                first_revision_number = Some(response.revisions[0].revision_number);
            }

            // 2. Compose new remote revisions together into a single change set. We will use this
            //    composed change set to transform our pending local revisions later.
            let composed_batch = ot::compose_iter(
                response
                    .revisions
                    .iter()
                    .map(|rev| rev.change_set.as_ref().unwrap()),
            )
            .map_err(CommittedLogError::OtError)?;
            composed_change_sets = ot::compose(&composed_change_sets, &composed_batch)
                .map_err(CommittedLogError::OtError)?;

            // 3. Add new remote revisions to the committed log. Their revision numbers should be
            //    consecutive integers.
            let mut inner = inner.lock().unwrap();
            for document_revision in response.revisions.into_iter() {
                let current_last_revision_number = inner.last_revision_number();
                if document_revision.revision_number == 1 + current_last_revision_number {
                    inner.revisions.push(document_revision);
                } else {
                    return Err(CommittedLogError::InvalidStateError(format!(
                        "Received new remote revision number {}, but expected {}",
                        document_revision.revision_number,
                        current_last_revision_number + 1
                    )));
                }
            }

            // 4. Set last_revision_number for next query. TODO(cliff): Should we trust this field
            //    in the response? Or should we just use the last revision's revision number to
            //    start the next query?
            last_revision_number = response.last_revision_number;
            if response.end_of_revisions {
                break;
            }
        }
        if first_revision_number.is_none() {
            // No new remote revisions found.
            return Ok(None);
        }
        // Some remote revisions were found. Return them composed into a single change set.
        Ok(Some(ComposedRemoteRevisions {
            composed_change_sets,
            revision_range: (first_revision_number.unwrap(), last_revision_number),
        }))
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let mut ret = Vec::new();
        let inner = self.inner.lock().unwrap();
        for revision in inner.revisions.iter() {
            ret.push(format!(
                "remote revision: {}",
                get_change_set_description(revision.change_set.as_ref().unwrap())
            ));
        }
        ret
    }
}

impl CommittedLogInner {
    /// If there is at least one revision in the committed log, return the revision number of the
    /// last revision. Otherwise, return 0.
    fn last_revision_number(&self) -> i64 {
        self.revisions
            .last()
            .as_ref()
            .map(|r| r.revision_number)
            .unwrap_or_else(|| 0)
    }
}
