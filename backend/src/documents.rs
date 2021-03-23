// TODO(cliff): Remove once this is called by request handlers.
#![allow(dead_code)]

use anyhow::Context;
use bytes::Bytes;
use prost::Message;
use rusoto_core::RusotoError;
use rusoto_dynamodb::{DynamoDb, DynamoDbClient, PutItemError, PutItemInput, QueryInput};

use crate::dynamodb::{av_b, av_get_b, av_get_n, av_get_s, av_map, av_n, av_s, table_name};
use crate::proto;
use crate::proto::writing::{
    submit_document_change_set_response::ResponseCode, ChangeSet, DocumentRevision,
    GetDocumentRevisionsRequest, GetDocumentRevisionsResponse, SubmitDocumentChangeSetRequest,
    SubmitDocumentChangeSetResponse,
};
use crate::utils::time;

async fn get_document_revisions(
    dynamodb_client: &DynamoDbClient,
    request: &GetDocumentRevisionsRequest,
) -> anyhow::Result<GetDocumentRevisionsResponse> {
    // error context: attach to errors for easier debugging
    let ec = || {
        format!(
            "[get_document_revisions] [ord_id: {}, doc_id: {}, after_revision_number: {}]",
            &request.org_id, &request.doc_id, request.after_revision_number
        )
    };
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
    let output = dynamodb_client.query(input).await.with_context(ec)?;
    let mut response = GetDocumentRevisionsResponse {
        revision_number: 0,
        revisions: Vec::new(),
    };
    if output.items.is_none() {
        return Ok(response);
    }
    let items = output.items.unwrap();
    if items.is_empty() {
        return Ok(response);
    }

    for item in items.into_iter() {
        let revision_number = av_get_n(&item, "revision_number")
            .ok_or("Missing revision_number")
            .map_err(anyhow::Error::msg)
            .with_context(ec)?;
        let change_set_binary = av_get_b(&item, "change_set")
            .ok_or("Missing change_set")
            .map_err(anyhow::Error::msg)
            .with_context(ec)?;
        let committed_at = av_get_s(&item, "committed_at")
            .ok_or("Missing committed_at")
            .map_err(anyhow::Error::msg)
            .with_context(ec)?;
        let change_set = ChangeSet::decode(&change_set_binary[..]).with_context(ec)?;
        response.revisions.push(DocumentRevision {
            doc_id: request.doc_id.clone(),
            revision_number,
            change_set: Some(change_set),
            committed_at: String::from(committed_at),
        });
        response.revision_number = revision_number;
    }
    Ok(response)
}

async fn submit_document_change_set(
    dynamodb_client: &DynamoDbClient,
    request: &SubmitDocumentChangeSetRequest,
) -> anyhow::Result<SubmitDocumentChangeSetResponse> {
    // error context: attach to errors for easier debugging
    let ec = || {
        format!(
            "[submit_document_change_set] [org_id: {}, doc_id: {}, on_revision_number: {}]",
            &request.org_id, &request.doc_id, request.on_revision_number
        )
    };
    let change_set = request
        .change_set
        .as_ref()
        .ok_or("change_set required")
        .map_err(anyhow::Error::msg)
        .with_context(ec)?;
    let change_set_binary = proto::encode_protobuf_message(change_set).with_context(ec)?;
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
            new_revision_number,
            newly_discovered_revisions: Vec::new(),
        }),
        Err(RusotoError::Service(PutItemError::ConditionalCheckFailed(_))) => {
            log::info!(
                "{} - Conditional check failed. Another revision was committed before ours. \
                Getting new revisions.",
                &ec()
            );
            let rev_request = GetDocumentRevisionsRequest {
                org_id: request.org_id.clone(),
                doc_id: request.doc_id.clone(),
                after_revision_number: request.on_revision_number,
            };
            let response = get_document_revisions(dynamodb_client, &rev_request)
                .await
                .with_context(ec)?;
            Ok(SubmitDocumentChangeSetResponse {
                response_code: ResponseCode::NewlyDiscoveredRevisions.into(),
                new_revision_number: response.revision_number,
                newly_discovered_revisions: response.revisions,
            })
        }
        Err(e) => Err(e).with_context(ec),
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

    #[tokio::test]
    async fn test_get_document_revisions() -> anyhow::Result<()> {
        let db = TestDynamoDb::new().await;

        let change_set1 = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Insert(Insert {
                        content: String::from("foo bar"),
                    })),
                }
            ],
        };
        let change_set2 = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Retain(Retain {
                        count: 3,
                    })),
                },
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Delete(Delete {
                        count: 4,
                    })),
                },
            ],
        };
        let change_set_bytes1 = Bytes::from(
            proto::encode_protobuf_message(&change_set1)?
        );
        let change_set_bytes2 = Bytes::from(
            proto::encode_protobuf_message(&change_set2)?
        );

        let doc_id1 = Id::new(IdType::Document);
        let org_id1 = Id::new(IdType::Organization);

        let doc_id2 = Id::new(IdType::Document);

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

        let response = get_document_revisions(
            &db.dynamodb_client,
            &GetDocumentRevisionsRequest {
                doc_id: String::from(doc_id1.as_str()),
                org_id: String::from(org_id1.as_str()),
                after_revision_number: 0,
            }
        ).await?;

        assert_eq!(response.revision_number, 2);
        assert_eq!(response.revisions.len(), 2);
        assert_eq!(&response.revisions[0].doc_id, doc_id1.as_str());
        assert_eq!(response.revisions[0].revision_number, 1);
        assert_eq!(response.revisions[0].change_set.as_ref().unwrap(), &change_set1);
        assert_eq!(
            &response.revisions[0].committed_at,
            &time::date_time_iso_str(&dt1)
        );
        assert_eq!(&response.revisions[1].doc_id, doc_id1.as_str());
        assert_eq!(response.revisions[1].revision_number, 2);
        assert_eq!(response.revisions[1].change_set.as_ref().unwrap(), &change_set2);
        assert_eq!(
            &response.revisions[1].committed_at,
            &time::date_time_iso_str(&dt2)
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_submit_change_set_success() -> anyhow::Result<()> {
        let db = TestDynamoDb::new().await;

        let existing_change_set = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Insert(Insert {
                        content: String::from("foo bar"),
                    })),
                }
            ],
        };
        let new_change_set = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Retain(Retain {
                        count: 3,
                    })),
                },
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Delete(Delete {
                        count: 4,
                    })),
                },
            ],
        };
        let existing_change_set_bytes = Bytes::from(
            proto::encode_protobuf_message(&existing_change_set)?
        );

        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);

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
            &SubmitDocumentChangeSetRequest {
                doc_id: String::from(doc_id.as_str()),
                org_id: String::from(org_id.as_str()),
                on_revision_number: 1,
                change_set: Some(new_change_set.clone()),
            }
        ).await?;

        assert_eq!(response.response_code(), ResponseCode::Ack);
        assert_eq!(response.new_revision_number, 2);
        assert!(response.newly_discovered_revisions.is_empty());

        let response = get_document_revisions(
            &db.dynamodb_client,
            &GetDocumentRevisionsRequest {
                doc_id: String::from(doc_id.as_str()),
                org_id: String::from(org_id.as_str()),
                after_revision_number: 1,
            }
        ).await?;

        assert_eq!(response.revision_number, 2);
        assert_eq!(response.revisions.len(), 1);
        assert_eq!(response.revisions[0].change_set.as_ref().unwrap(), &new_change_set);

        Ok(())
    }

    #[tokio::test]
    async fn test_submit_change_set_collision() -> anyhow::Result<()> {
        let db = TestDynamoDb::new().await;

        let change_set1 = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Insert(Insert {
                        content: String::from("foo bar"),
                    })),
                }
            ],
        };
        let change_set2 = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Retain(Retain {
                        count: 3,
                    })),
                },
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Delete(Delete {
                        count: 4,
                    })),
                },
            ],
        };
        let new_change_set = ChangeSet {
            ops: vec![
                ChangeOp {
                    change_op: Some(change_op::ChangeOp::Delete(Delete {
                        count: 4,
                    })),
                },
            ],
        };
        let change_set_bytes1 = Bytes::from(
            proto::encode_protobuf_message(&change_set1)?
        );
        let change_set_bytes2 = Bytes::from(
            proto::encode_protobuf_message(&change_set2)?
        );

        let doc_id = Id::new(IdType::Document);
        let org_id = Id::new(IdType::Organization);

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

        let response = submit_document_change_set(
            &db.dynamodb_client,
            &SubmitDocumentChangeSetRequest {
                doc_id: String::from(doc_id.as_str()),
                org_id: String::from(org_id.as_str()),
                on_revision_number: 1,
                change_set: Some(new_change_set.clone()),
            }
        ).await?;

        assert_eq!(response.response_code(), ResponseCode::NewlyDiscoveredRevisions);
        assert_eq!(response.new_revision_number, 2);
        assert_eq!(response.newly_discovered_revisions.len(), 1);
        assert_eq!(&response.newly_discovered_revisions[0].doc_id, doc_id.as_str());
        assert_eq!(response.newly_discovered_revisions[0].revision_number, 2);
        assert_eq!(response.newly_discovered_revisions[0].change_set.as_ref().unwrap(), &change_set2);
        assert_eq!(
            &response.newly_discovered_revisions[0].committed_at,
            &time::date_time_iso_str(&dt2)
        );

        Ok(())
    }
}
