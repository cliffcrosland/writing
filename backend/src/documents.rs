use std::convert::{TryFrom, TryInto};

use actix_web::error;
use bytes::Bytes;
use prost::Message;
use rusoto_core::RusotoError;
use rusoto_dynamodb::{DynamoDb, DynamoDbClient, PutItemError, PutItemInput, QueryInput};

use crate::dynamodb::{av_b, av_get_b, av_get_n, av_get_s, av_map, av_n, av_s, table_name};
use crate::http::SessionUser;
use crate::proto;
use crate::proto::writing::{
    submit_document_change_set_response::ResponseCode, ChangeSet, DocumentRevision,
    GetDocumentRevisionsRequest, GetDocumentRevisionsResponse, SubmitDocumentChangeSetRequest,
    SubmitDocumentChangeSetResponse,
};
use crate::utils::time;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SharingPermission {
    No = 0,
    Read = 1,
    ReadAndWrite = 2,
}

impl TryFrom<i32> for SharingPermission {
    type Error = ();

    fn try_from(val: i32) -> Result<Self, Self::Error> {
        match val {
            0 => Ok(SharingPermission::No),
            1 => Ok(SharingPermission::Read),
            2 => Ok(SharingPermission::ReadAndWrite),
            _ => Err(()),
        }
    }
}

pub async fn get_document_revisions(
    dynamodb_client: &DynamoDbClient,
    session_user: &SessionUser,
    request: &GetDocumentRevisionsRequest,
) -> actix_web::Result<GetDocumentRevisionsResponse> {
    validate_user_has_some_permission(
        &dynamodb_client,
        &session_user,
        &request.doc_id,
        &request.org_id,
        &[SharingPermission::Read, SharingPermission::ReadAndWrite],
    )
    .await?;

    let input = QueryInput {
        table_name: table_name("document_revisions"),
        // Need consistent read to make sure we wait for pending writes to the revision log to
        // finish. Prevents us from seeing gaps in the log.
        consistent_read: Some(true),
        key_condition_expression: Some(String::from(
            "doc_id = :doc_id AND revision_number > :after_revision_number",
        )),
        expression_attribute_values: Some(av_map(&[
            av_s(":doc_id", &request.doc_id),
            av_n(":after_revision_number", request.after_revision_number),
        ])),
        projection_expression: Some(String::from("revision_number, change_set, committed_at")),
        ..Default::default()
    };
    let output = dynamodb_client.query(input).await.map_err(|e| {
        log::error!("{}", e);
        error::ErrorInternalServerError("")
    })?;
    let mut response = GetDocumentRevisionsResponse {
        last_revision_number: 0,
        revisions: Vec::new(),
        end_of_revisions: output.last_evaluated_key.is_none(),
    };
    if output.items.is_none() {
        return Ok(response);
    }
    let items = output.items.unwrap();
    if items.is_empty() {
        return Ok(response);
    }

    let missing_field_error = || {
        log::error!(
            "document_revision is missing a field! doc_id: {}",
            &request.doc_id
        );
        error::ErrorInternalServerError("")
    };
    for item in items.into_iter() {
        let revision_number = av_get_n(&item, "revision_number").ok_or_else(missing_field_error)?;
        let change_set_binary = av_get_b(&item, "change_set").ok_or_else(missing_field_error)?;
        let committed_at = av_get_s(&item, "committed_at").ok_or_else(missing_field_error)?;
        let change_set = ChangeSet::decode(&change_set_binary[..]).map_err(|e| {
            log::error!("{}", e);
            error::ErrorInternalServerError("")
        })?;
        response.revisions.push(DocumentRevision {
            doc_id: request.doc_id.clone(),
            revision_number,
            change_set: Some(change_set),
            committed_at: String::from(committed_at),
        });
        response.last_revision_number = revision_number;
    }

    Ok(response)
}

pub async fn submit_document_change_set(
    dynamodb_client: &DynamoDbClient,
    session_user: &SessionUser,
    request: &SubmitDocumentChangeSetRequest,
) -> actix_web::Result<SubmitDocumentChangeSetResponse> {
    validate_user_has_some_permission(
        dynamodb_client,
        session_user,
        &request.doc_id,
        &request.org_id,
        &[SharingPermission::ReadAndWrite],
    )
    .await?;

    let change_set = request
        .change_set
        .as_ref()
        .ok_or_else(|| error::ErrorBadRequest(""))?;
    let change_set_binary = proto::encode_protobuf_message(change_set).map_err(|e| {
        log::error!("{}", e);
        error::ErrorBadRequest("")
    })?;
    let change_set_binary = Bytes::from(change_set_binary);
    let new_revision_number = request.on_revision_number + 1;
    let input = PutItemInput {
        table_name: table_name("document_revisions"),
        item: av_map(&[
            av_s("doc_id", &request.doc_id),
            av_n("revision_number", new_revision_number),
            av_b("change_set", change_set_binary),
            av_s(
                "committed_at",
                &time::date_time_iso_str(&chrono::Utc::now()),
            ),
        ]),
        // Only succeed if key (doc_id, revision_number) does not already exist.
        condition_expression: Some(String::from(
            "attribute_not_exists(doc_id) AND attribute_not_exists(revision_number)",
        )),
        ..Default::default()
    };
    let result = dynamodb_client.put_item(input).await;
    match result {
        Ok(_) => Ok(SubmitDocumentChangeSetResponse {
            response_code: ResponseCode::Ack.into(),
            last_revision_number: new_revision_number,
            revisions: Vec::new(),
            end_of_revisions: true,
        }),
        Err(RusotoError::Service(PutItemError::ConditionalCheckFailed(_))) => {
            log::info!(
                "doc_id: {} - Conditional check failed. Another revision was committed before ours. \
                Getting new revisions.",
                &request.doc_id,
            );
            let rev_request = GetDocumentRevisionsRequest {
                org_id: request.org_id.clone(),
                doc_id: request.doc_id.clone(),
                after_revision_number: request.on_revision_number,
            };
            let response =
                get_document_revisions(dynamodb_client, session_user, &rev_request).await?;
            Ok(SubmitDocumentChangeSetResponse {
                response_code: ResponseCode::DiscoveredNewRevisions.into(),
                last_revision_number: response.last_revision_number,
                revisions: response.revisions,
                end_of_revisions: response.end_of_revisions,
            })
        }
        Err(e) => {
            log::error!("{}", e);
            Err(error::ErrorInternalServerError(""))
        }
    }
}

async fn validate_user_has_some_permission(
    dynamodb_client: &DynamoDbClient,
    session_user: &SessionUser,
    doc_id: &str,
    doc_org_id: &str,
    permissions: &[SharingPermission],
) -> actix_web::Result<()> {
    // 1. I must belong to the org where the document is stored. Documents in other orgs are
    //    invisible to me.
    if session_user.org_id.as_str() != doc_org_id {
        return Err(error::ErrorNotFound(""));
    }

    // 2. The document must exist.
    let input = QueryInput {
        table_name: table_name("documents"),
        key_condition_expression: Some(String::from("id = :doc_id")),
        filter_expression: Some(String::from("org_id = :org_id")),
        projection_expression: Some(String::from(
            "created_by_user_id, org_level_sharing_permission",
        )),
        expression_attribute_values: Some(av_map(&[
            av_s(":doc_id", doc_id),
            av_s(":org_id", doc_org_id),
        ])),
        ..Default::default()
    };
    let output = dynamodb_client.query(input).await.map_err(|e| {
        log::error!("{}", e);
        error::ErrorInternalServerError("")
    })?;
    if output.items.is_none() || output.count.is_none() || output.count.unwrap() != 1 {
        return Err(error::ErrorNotFound(""));
    }
    let items = output.items.unwrap();
    let item = items.first().ok_or_else(|| error::ErrorNotFound(""))?;

    // 3. If I created this document, then I have permission.
    // TODO(cliff): Is there anything bad about this rule? Seems pretty powerful.
    let created_by_user_id =
        av_get_s(item, "created_by_user_id").ok_or_else(|| error::ErrorNotFound(""))?;
    if created_by_user_id == session_user.user_id.as_str() {
        return Ok(());
    }

    // 4. If the document was shared with the entire org, check to see if that gave me
    //    permission.
    let org_level_sharing_permission: i32 =
        av_get_n(item, "org_level_sharing_permission").ok_or_else(|| error::ErrorNotFound(""))?;
    let org_level_sharing_permission: SharingPermission = org_level_sharing_permission
        .try_into()
        .map_err(|_| error::ErrorForbidden(""))?;
    let found_permission_match = permissions
        .iter()
        .any(|p| p == &org_level_sharing_permission);
    if found_permission_match {
        return Ok(());
    }

    // 5. If the document was shared with me, check to see if that gave me permission.
    let input = QueryInput {
        table_name: table_name("document_user_sharing_permissions"),
        key_condition_expression: Some(String::from("doc_id = :doc_id AND user_id = :user_id")),
        filter_expression: Some(String::from("org_id = :org_id")),
        expression_attribute_values: Some(av_map(&[
            av_s(":doc_id", doc_id),
            av_s(":user_id", session_user.user_id.as_str()),
            av_s(":org_id", doc_org_id),
        ])),
        projection_expression: Some(String::from("sharing_permission")),
        ..Default::default()
    };
    let output = dynamodb_client.query(input).await.map_err(|e| {
        log::error!("{}", e);
        error::ErrorInternalServerError("")
    })?;
    if output.items.is_none() || output.count.is_none() || output.count.unwrap() != 1 {
        return Err(error::ErrorForbidden(""));
    }
    let items = output.items.unwrap();
    let item = items.first().ok_or_else(|| error::ErrorForbidden(""))?;
    let sharing_permission: i32 =
        av_get_n(item, "sharing_permission").ok_or_else(|| error::ErrorForbidden(""))?;
    let sharing_permission: SharingPermission = sharing_permission
        .try_into()
        .map_err(|_| error::ErrorForbidden(""))?;
    let found_permission_match = permissions.iter().any(|p| p == &sharing_permission);
    if found_permission_match {
        Ok(())
    } else {
        Err(error::ErrorForbidden(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;
    use std::ops::Sub;

    use rusoto_dynamodb::AttributeValue;

    use crate::ids::{Id, IdType};
    use crate::proto::writing::{change_op, ChangeOp, ChangeSet, Delete, Insert, Retain};
    use crate::testing::utils::TestDynamoDb;
    use crate::users::UserRole;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn test_get_document_revisions() -> TestResult {
        let db = TestDynamoDb::new().await;

        // Create 2 different change sets.
        let change_set1 = ChangeSet {
            ops: vec![ChangeOp {
                change_op: Some(change_op::ChangeOp::Insert(Insert {
                    content: String::from("foo bar"),
                })),
            }],
        };
        let change_set2 = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Retain(Retain { count: 3 })),
                },
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Delete(Delete { count: 4 })),
                },
            ],
        };
        let change_set_bytes1 = Bytes::from(proto::encode_protobuf_message(&change_set1)?);
        let change_set_bytes2 = Bytes::from(proto::encode_protobuf_message(&change_set2)?);

        // Document 1:
        let doc_id1 = Id::new(IdType::Document);
        let org_id1 = Id::new(IdType::Organization);
        let user_id1 = Id::new(IdType::User);
        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id1.clone(),
                org_id: org_id1.clone(),
                created_by_user_id: user_id1.clone(),
                org_level_sharing_permission: SharingPermission::ReadAndWrite,
            },
        )
        .await?;
        let session_user1 = SessionUser {
            user_id: user_id1.clone(),
            org_id: org_id1.clone(),
            role: UserRole::Default,
        };

        // Document 2:
        let doc_id2 = Id::new(IdType::Document);
        let org_id2 = Id::new(IdType::Document);
        let user_id2 = Id::new(IdType::User);
        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id2.clone(),
                org_id: org_id2.clone(),
                created_by_user_id: user_id2.clone(),
                org_level_sharing_permission: SharingPermission::ReadAndWrite,
            },
        )
        .await?;

        // Add revisions to Document 1 and 2.
        let dt1 = chrono::Utc::now().sub(chrono::Duration::days(7));
        let dt2 = chrono::Utc::now().sub(chrono::Duration::days(6));
        let dt3 = chrono::Utc::now().sub(chrono::Duration::days(5));

        let items: Vec<HashMap<String, AttributeValue>> = vec![
            av_map(&[
                av_s("doc_id", doc_id1.as_str()),
                av_n("revision_number", 1),
                av_b("change_set", change_set_bytes1.clone()),
                av_s("committed_at", &time::date_time_iso_str(&dt1)),
            ]),
            av_map(&[
                av_s("doc_id", doc_id1.as_str()),
                av_n("revision_number", 2),
                av_b("change_set", change_set_bytes2.clone()),
                av_s("committed_at", &time::date_time_iso_str(&dt2)),
            ]),
            av_map(&[
                av_s("doc_id", doc_id2.as_str()),
                av_n("revision_number", 1),
                av_b("change_set", change_set_bytes1.clone()),
                av_s("committed_at", &time::date_time_iso_str(&dt3)),
            ]),
        ];
        for item in items.into_iter() {
            let input = PutItemInput {
                table_name: table_name("document_revisions"),
                item,
                ..Default::default()
            };
            db.dynamodb_client.put_item(input).await?;
        }

        // Get revisions for Document 1
        let response = get_document_revisions(
            &db.dynamodb_client,
            &session_user1,
            &GetDocumentRevisionsRequest {
                doc_id: String::from(doc_id1.as_str()),
                org_id: String::from(org_id1.as_str()),
                after_revision_number: 0,
            },
        )
        .await?;

        assert_eq!(response.last_revision_number, 2);
        assert_eq!(response.revisions.len(), 2);
        assert_eq!(&response.revisions[0].doc_id, doc_id1.as_str());
        assert_eq!(response.revisions[0].revision_number, 1);
        assert_eq!(
            response.revisions[0].change_set.as_ref().unwrap(),
            &change_set1
        );
        assert_eq!(
            &response.revisions[0].committed_at,
            &time::date_time_iso_str(&dt1)
        );
        assert_eq!(&response.revisions[1].doc_id, doc_id1.as_str());
        assert_eq!(response.revisions[1].revision_number, 2);
        assert_eq!(
            response.revisions[1].change_set.as_ref().unwrap(),
            &change_set2
        );
        assert_eq!(
            &response.revisions[1].committed_at,
            &time::date_time_iso_str(&dt2)
        );
        assert!(response.end_of_revisions);

        Ok(())
    }

    #[tokio::test]
    async fn test_submit_change_set_success() -> TestResult {
        let db = TestDynamoDb::new().await;

        // Create a document, and a user who can read from and write to it.
        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);
        let user_id = Id::new(IdType::User);
        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id.clone(),
                org_id: org_id.clone(),
                created_by_user_id: user_id.clone(),
                org_level_sharing_permission: SharingPermission::ReadAndWrite,
            },
        )
        .await?;
        let session_user = SessionUser {
            user_id: user_id.clone(),
            org_id: org_id.clone(),
            role: UserRole::Default,
        };

        // Add a change set to the revision log. Prepare a new change set to be submitted by the
        // user.
        let existing_change_set = ChangeSet {
            ops: vec![ChangeOp {
                change_op: Some(change_op::ChangeOp::Insert(Insert {
                    content: String::from("foo bar"),
                })),
            }],
        };
        let new_change_set = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Retain(Retain { count: 3 })),
                },
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Delete(Delete { count: 4 })),
                },
            ],
        };
        let existing_change_set_bytes =
            Bytes::from(proto::encode_protobuf_message(&existing_change_set)?);

        let dt1 = chrono::Utc::now().sub(chrono::Duration::days(7));

        let input = PutItemInput {
            table_name: table_name("document_revisions"),
            item: av_map(&[
                av_s("doc_id", doc_id.as_str()),
                av_n("revision_number", 1),
                av_b("change_set", existing_change_set_bytes.clone()),
                av_s("committed_at", &time::date_time_iso_str(&dt1)),
            ]),
            ..Default::default()
        };
        db.dynamodb_client.put_item(input).await?;

        let response = submit_document_change_set(
            &db.dynamodb_client,
            &session_user,
            &SubmitDocumentChangeSetRequest {
                doc_id: String::from(doc_id.as_str()),
                org_id: String::from(org_id.as_str()),
                on_revision_number: 1,
                change_set: Some(new_change_set.clone()),
            },
        )
        .await?;

        assert_eq!(response.response_code(), ResponseCode::Ack);
        assert_eq!(response.last_revision_number, 2);
        assert!(response.revisions.is_empty());

        let response = get_document_revisions(
            &db.dynamodb_client,
            &session_user,
            &GetDocumentRevisionsRequest {
                doc_id: String::from(doc_id.as_str()),
                org_id: String::from(org_id.as_str()),
                after_revision_number: 1,
            },
        )
        .await?;

        assert_eq!(response.last_revision_number, 2);
        assert_eq!(response.revisions.len(), 1);
        assert_eq!(
            response.revisions[0].change_set.as_ref().unwrap(),
            &new_change_set
        );
        assert!(response.end_of_revisions);

        Ok(())
    }

    #[tokio::test]
    async fn test_submit_change_set_collision() -> TestResult {
        let db = TestDynamoDb::new().await;

        let change_set1 = ChangeSet {
            ops: vec![ChangeOp {
                change_op: Some(change_op::ChangeOp::Insert(Insert {
                    content: String::from("foo bar"),
                })),
            }],
        };
        let change_set2 = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Retain(Retain { count: 3 })),
                },
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Delete(Delete { count: 4 })),
                },
            ],
        };
        let new_change_set = ChangeSet {
            ops: vec![ChangeOp {
                change_op: Some(change_op::ChangeOp::Delete(Delete { count: 4 })),
            }],
        };
        let change_set_bytes1 = Bytes::from(proto::encode_protobuf_message(&change_set1)?);
        let change_set_bytes2 = Bytes::from(proto::encode_protobuf_message(&change_set2)?);

        // Create a document, and a user who can read it, write it.
        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);
        let user_id = Id::new(IdType::User);
        let session_user = SessionUser {
            user_id: user_id.clone(),
            org_id: org_id.clone(),
            role: UserRole::Default,
        };
        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id.clone(),
                org_id: org_id.clone(),
                created_by_user_id: user_id.clone(),
                org_level_sharing_permission: SharingPermission::ReadAndWrite,
            },
        )
        .await?;

        // Add two revisions to the document. The latest revision number is 2.
        let dt1 = chrono::Utc::now().sub(chrono::Duration::days(1));
        let dt2 = chrono::Utc::now().sub(chrono::Duration::seconds(10));

        let items: Vec<HashMap<String, AttributeValue>> = vec![
            av_map(&[
                av_s("doc_id", doc_id.as_str()),
                av_n("revision_number", 1),
                av_b("change_set", change_set_bytes1.clone()),
                av_s("committed_at", &time::date_time_iso_str(&dt1)),
            ]),
            av_map(&[
                av_s("doc_id", doc_id.as_str()),
                av_n("revision_number", 2),
                av_b("change_set", change_set_bytes2.clone()),
                av_s("committed_at", &time::date_time_iso_str(&dt2)),
            ]),
        ];
        for item in items.into_iter() {
            let input = PutItemInput {
                table_name: table_name("document_revisions"),
                item,
                ..Default::default()
            };
            db.dynamodb_client.put_item(input).await?;
        }

        // Attempt to submit a new revision on top of revision 1. This should fail because revision
        // 1 is no longer the latest revision. The function will return the new revision.
        let response = submit_document_change_set(
            &db.dynamodb_client,
            &session_user,
            &SubmitDocumentChangeSetRequest {
                doc_id: String::from(doc_id.as_str()),
                org_id: String::from(org_id.as_str()),
                on_revision_number: 1,
                change_set: Some(new_change_set.clone()),
            },
        )
        .await?;

        assert_eq!(
            response.response_code(),
            ResponseCode::DiscoveredNewRevisions
        );
        assert_eq!(response.last_revision_number, 2);
        assert_eq!(response.revisions.len(), 1);
        assert_eq!(&response.revisions[0].doc_id, doc_id.as_str());
        assert_eq!(response.revisions[0].revision_number, 2);
        assert_eq!(
            response.revisions[0].change_set.as_ref().unwrap(),
            &change_set2
        );
        assert_eq!(
            &response.revisions[0].committed_at,
            &time::date_time_iso_str(&dt2)
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_permission_created_by_user() -> TestResult {
        let db = TestDynamoDb::new().await;

        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);
        let created_by_user_id = Id::new(IdType::User);

        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id.clone(),
                org_id: org_id.clone(),
                created_by_user_id: created_by_user_id.clone(),
                org_level_sharing_permission: SharingPermission::No,
            },
        )
        .await?;

        // User requests permission to read a document she created. Should be accepted.
        let mut session_user = SessionUser {
            user_id: created_by_user_id.clone(),
            org_id: org_id.clone(),
            role: UserRole::Default,
        };

        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id.as_str(),
            &[SharingPermission::Read],
        )
        .await;

        assert!(result.is_ok());

        // User requests permission to read a document created by another user in her org. The
        // document does not have any org-level sharing, and it was no explicitly shared with the
        // user. Should be rejected.
        session_user.user_id = Id::new(IdType::User);
        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id.as_str(),
            &[SharingPermission::Read],
        )
        .await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        let response_error = error.as_response_error();
        // 403 Forbidden
        assert_eq!(response_error.status_code(), 403);

        Ok(())
    }

    #[tokio::test]
    async fn test_permission_org_level() -> TestResult {
        let db = TestDynamoDb::new().await;

        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);
        let created_by_user_id = Id::new(IdType::User);

        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id.clone(),
                org_id: org_id.clone(),
                created_by_user_id: created_by_user_id.clone(),
                org_level_sharing_permission: SharingPermission::Read,
            },
        )
        .await?;

        // User requested permission to read a doc created by someone in her org. The org-level
        // permission is Read, so should be accepted.
        let session_user = SessionUser {
            user_id: Id::new(IdType::User),
            org_id: org_id.clone(),
            role: UserRole::Default,
        };

        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id.as_str(),
            &[SharingPermission::Read],
        )
        .await;

        assert!(result.is_ok());

        // User requested permission to read and write a doc created by someone in her org. The
        // org-level permission is Read only, and the document was not explicitly shared with the
        // user. Should be rejected.
        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id.as_str(),
            &[SharingPermission::ReadAndWrite],
        )
        .await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        let response_error = error.as_response_error();
        // 403 Forbidden
        assert_eq!(response_error.status_code(), 403);

        Ok(())
    }

    #[tokio::test]
    async fn test_permission_user_level() -> TestResult {
        let db = TestDynamoDb::new().await;

        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);
        let created_by_user_id = Id::new(IdType::User);
        let reader_user_id = Id::new(IdType::User);

        // Document created with no org-level sharing permission.
        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id.clone(),
                org_id: org_id.clone(),
                created_by_user_id: created_by_user_id.clone(),
                org_level_sharing_permission: SharingPermission::No,
            },
        )
        .await?;

        // User is specifically given permission to read the document.
        create_document_user_sharing_permission(
            &db.dynamodb_client,
            DocumentUserSharingPermissionParams {
                doc_id: doc_id.clone(),
                user_id: reader_user_id.clone(),
                org_id: org_id.clone(),
                sharing_permission: SharingPermission::Read,
            },
        )
        .await?;

        // User requested to read the document. Should be accepted.
        let session_user = SessionUser {
            user_id: reader_user_id.clone(),
            org_id: org_id.clone(),
            role: UserRole::Default,
        };

        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id.as_str(),
            &[SharingPermission::Read],
        )
        .await;

        assert!(result.is_ok());

        // User requested permission to read *and* write the document. She was specifically given
        // permission to read the document, but *not* to write. Should be rejected.
        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id.as_str(),
            &[SharingPermission::ReadAndWrite],
        )
        .await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        let response_error = error.as_response_error();
        // 403 Forbidden
        assert_eq!(response_error.status_code(), 403);

        Ok(())
    }

    #[tokio::test]
    async fn test_permission_document_is_in_different_org() -> TestResult {
        let db = TestDynamoDb::new().await;

        let doc_id = Id::new(IdType::Document);
        let org_id1 = Id::new(IdType::Organization);
        let created_by_user_id = Id::new(IdType::User);

        create_document(
            &db.dynamodb_client,
            DocParams {
                doc_id: doc_id.clone(),
                org_id: org_id1.clone(),
                created_by_user_id: created_by_user_id.clone(),
                org_level_sharing_permission: SharingPermission::Read,
            },
        )
        .await?;

        // User requested permission to read a doc in a different org. Should get a 404 Not Found
        // error result.
        let org_id2 = Id::new(IdType::Organization);
        let session_user = SessionUser {
            user_id: Id::new(IdType::User),
            org_id: org_id2.clone(),
            role: UserRole::Default,
        };

        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id1.as_str(),
            &[SharingPermission::Read],
        )
        .await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        let response_error = error.as_response_error();
        // 404 Not Found
        assert_eq!(response_error.status_code(), 404);

        Ok(())
    }

    #[tokio::test]
    async fn test_permission_document_does_not_exist() -> TestResult {
        let db = TestDynamoDb::new().await;

        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);

        // User requested permission to read a non-existent document. Should get a
        // 404 Not Found.
        let session_user = SessionUser {
            user_id: Id::new(IdType::User),
            org_id: org_id.clone(),
            role: UserRole::Default,
        };

        let result = validate_user_has_some_permission(
            &db.dynamodb_client,
            &session_user,
            doc_id.as_str(),
            org_id.as_str(),
            &[SharingPermission::Read],
        )
        .await;

        assert!(result.is_err());
        let error = result.err().unwrap();
        let response_error = error.as_response_error();
        // 404 Not Found
        assert_eq!(response_error.status_code(), 404);

        Ok(())
    }

    struct DocParams {
        doc_id: Id,
        org_id: Id,
        created_by_user_id: Id,
        org_level_sharing_permission: SharingPermission,
    }

    async fn create_document(
        dynamodb_client: &DynamoDbClient,
        doc_params: DocParams,
    ) -> TestResult {
        let doc_created_at = chrono::Utc::now().sub(chrono::Duration::days(1));
        let doc_created_at_str = time::date_time_iso_str(&doc_created_at);
        let input = rusoto_dynamodb::PutItemInput {
            table_name: table_name("documents"),
            item: av_map(&[
                av_s("id", doc_params.doc_id.as_str()),
                av_s("org_id", doc_params.org_id.as_str()),
                av_s("title", "My favorite document ever"),
                av_s("created_by_user_id", doc_params.created_by_user_id.as_str()),
                av_n(
                    "org_level_sharing_permission",
                    doc_params.org_level_sharing_permission as i32,
                ),
                av_s("created_at", &doc_created_at_str),
                av_s("updated_at", &doc_created_at_str),
            ]),
            ..Default::default()
        };
        dynamodb_client.put_item(input).await?;
        Ok(())
    }

    struct DocumentUserSharingPermissionParams {
        doc_id: Id,
        user_id: Id,
        org_id: Id,
        sharing_permission: SharingPermission,
    }

    async fn create_document_user_sharing_permission(
        dynamodb_client: &DynamoDbClient,
        params: DocumentUserSharingPermissionParams,
    ) -> TestResult {
        let created_at = chrono::Utc::now().sub(chrono::Duration::days(1));
        let created_at_str = time::date_time_iso_str(&created_at);
        let input = rusoto_dynamodb::PutItemInput {
            table_name: table_name("document_user_sharing_permissions"),
            item: av_map(&[
                av_s("doc_id", params.doc_id.as_str()),
                av_s("user_id", params.user_id.as_str()),
                av_s("org_id", params.org_id.as_str()),
                av_n("sharing_permission", params.sharing_permission as i32),
                av_s("created_at", &created_at_str),
                av_s("updated_at", &created_at_str),
            ]),
            ..Default::default()
        };
        dynamodb_client.put_item(input).await?;
        Ok(())
    }
}
