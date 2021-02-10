#[cfg(test)]

use futures::future;
use lazy_static::lazy_static;
use rusoto_dynamodb::{
    AttributeDefinition, CreateTableInput, DynamoDb, KeySchemaElement, LocalSecondaryIndex,
    Projection,
};

lazy_static! {
    static ref TABLE_DEFINITIONS: Vec<CreateTableInput> = vec![
        CreateTableInput {
            table_name: "users".to_string(),
            attribute_definitions: vec![
                AttributeDefinition {
                    attribute_name: "id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "email".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "hashed_password".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "name".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "photo_url".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "created_at".to_string(),
                    attribute_type: "N".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "updated_at".to_string(),
                    attribute_type: "N".to_string(),
                },
            ],
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "HASH".to_string(),
                }
            ],
            local_secondary_indexes: Some(vec![
                LocalSecondaryIndex {
                    index_name: "users_email".to_string(),
                    key_schema: vec![
                        KeySchemaElement {
                            attribute_name: "email".to_string(),
                            key_type: "HASH".to_string(),
                        }
                    ],
                    projection: Projection {
                        projection_type: Some("ALL".to_string()),
                        ..Default::default()
                    },
                },
            ]),
            ..Default::default()
        },
        CreateTableInput {
            table_name: "organizations".to_string(),
            attribute_definitions: vec![
                AttributeDefinition {
                    attribute_name: "id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "name".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "logo_url".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "created_at".to_string(),
                    attribute_type: "N".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "updated_at".to_string(),
                    attribute_type: "N".to_string(),
                },
            ],
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "HASH".to_string(),
                },
            ],
            ..Default::default()
        },
        CreateTableInput {
            table_name: "organization_users".to_string(),
            attribute_definitions: vec![
                AttributeDefinition {
                    attribute_name: "org_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "role".to_string(),
                    attribute_type: "N".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "created_at".to_string(),
                    attribute_type: "N".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "updated_at".to_string(),
                    attribute_type: "N".to_string(),
                },
            ],
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "org_id".to_string(),
                    key_type: "HASH".to_string(),
                },
                KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "RANGE".to_string(),
                }
            ],
            ..Default::default()
        },
        CreateTableInput {
            table_name: "pages".to_string(),
            attribute_definitions: vec![
                AttributeDefinition {
                    attribute_name: "org_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "title".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "created_by_user_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "current_page_revision_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "created_at".to_string(),
                    attribute_type: "N".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "updated_at".to_string(),
                    attribute_type: "N".to_string(),
                },
            ],
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "HASH".to_string(),
                },
            ],
            local_secondary_indexes: Some(vec![
                LocalSecondaryIndex {
                    index_name: "pages_by_title".to_string(),
                    key_schema: vec![
                        KeySchemaElement {
                            attribute_name: "org_id".to_string(),
                            key_type: "HASH".to_string(),
                        },
                        KeySchemaElement {
                            attribute_name: "title".to_string(),
                            key_type: "RANGE".to_string(),
                        },
                    ],
                    projection: Projection {
                        projection_type: Some("ALL".to_string()),
                        ..Default::default()
                    },
                },
                LocalSecondaryIndex {
                    index_name: "pages_created_by_user_id".to_string(),
                    key_schema: vec![
                        KeySchemaElement {
                            attribute_name: "created_by_user_id".to_string(),
                            key_type: "HASH".to_string(),
                        },
                        KeySchemaElement {
                            attribute_name: "created_at".to_string(),
                            key_type: "RANGE".to_string(),
                        },
                    ],
                    projection: Projection {
                        projection_type: Some("ALL".to_string()),
                        ..Default::default()
                    },
                },
            ]),
            ..Default::default()
        },
        CreateTableInput {
            table_name: "page_content_chunks".to_string(),
            attribute_definitions: vec![
                AttributeDefinition {
                    attribute_name: "org_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "page_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "content".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "chunk_index".to_string(),
                    attribute_type: "N".to_string(),
                },
            ],
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "page_id".to_string(),
                    key_type: "HASH".to_string(),
                },
                KeySchemaElement {
                    attribute_name: "chunk_index".to_string(),
                    key_type: "RANGE".to_string(),
                },
            ],
            ..Default::default()
        },
        CreateTableInput {
            table_name: "page_revisions".to_string(),
            attribute_definitions: vec![
                AttributeDefinition {
                    attribute_name: "org_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "page_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "page_edit".to_string(),
                    attribute_type: "B".to_string(),
                },
            ],
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "page_id".to_string(),
                    key_type: "HASH".to_string(),
                },
                KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "RANGE".to_string(),
                },
            ],
            ..Default::default()
        },
    ];
}

pub async fn create_test_tables(dynamodb_client: &dyn DynamoDb) {
    let mut futures = Vec::new();
    for table_def in TABLE_DEFINITIONS.iter().cloned() {
        let future = dynamodb_client.create_table(table_def);
        futures.push(future);
    }
    let results = future::join_all(futures).await;
    for result in results {
        assert!(result.is_ok());
    }
}

pub async fn delete_test_tables(dynamodb_client: &dyn DynamoDb) {
    let mut futures = Vec::new();
    for table_def in TABLE_DEFINITIONS.iter() {
        let future = dynamodb_client.delete_table(DeleteTableInput {
            table_name: table_def.table_name.clone(),
        });
        futures.push(future);
    }
    let results = future::join_all(futures).await;
    for result in results {
        assert!(result.is_ok());
    }
}
