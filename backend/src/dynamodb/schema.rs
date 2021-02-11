#[cfg(test)]
// Note: The DynamoDB schema in this file exists for the sake of documentation and testing. We do
// not compile the code in this file into the release binary.
//
// Tables in the staging and production environments are created and maintained manually in the AWS
// UI. We want to make important decisions about the tables using the AWS UI, not automatically
// (eg. table billing modes).
use lazy_static::lazy_static;
use rusoto_dynamodb::{
    AttributeDefinition, CreateTableInput, GlobalSecondaryIndex, KeySchemaElement, Projection,
};

lazy_static! {
    pub static ref TABLE_DEFINITIONS: Vec<CreateTableInput> = vec![
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
            key_schema: vec![KeySchemaElement {
                attribute_name: "email".to_string(),
                key_type: "HASH".to_string(),
            }],
            global_secondary_indexes: Some(vec![GlobalSecondaryIndex {
                index_name: "users_id-index".to_string(),
                key_schema: vec![KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "HASH".to_string(),
                }],
                projection: Projection {
                    projection_type: Some("ALL".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },]),
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
            key_schema: vec![KeySchemaElement {
                attribute_name: "id".to_string(),
                key_type: "HASH".to_string(),
            },],
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
                    attribute_name: "user_id".to_string(),
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
                AttributeDefinition {
                    attribute_name: "last_login_at".to_string(),
                    attribute_type: "N".to_string(),
                },
            ],
            key_schema: vec![
                KeySchemaElement {
                    attribute_name: "org_id".to_string(),
                    key_type: "HASH".to_string(),
                },
                KeySchemaElement {
                    attribute_name: "user_id".to_string(),
                    key_type: "RANGE".to_string(),
                }
            ],
            global_secondary_indexes: Some(vec![GlobalSecondaryIndex {
                index_name: "organization_users_user_id_last_login_at-index".to_string(),
                key_schema: vec![
                    KeySchemaElement {
                        attribute_name: "user_id".to_string(),
                        key_type: "HASH".to_string(),
                    },
                    KeySchemaElement {
                        attribute_name: "last_login_at".to_string(),
                        key_type: "RANGE".to_string(),
                    }
                ],
                projection: Projection {
                    projection_type: Some("ALL".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            }]),
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
                    attribute_name: "org_level_sharing_permission".to_string(),
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
            key_schema: vec![KeySchemaElement {
                attribute_name: "id".to_string(),
                key_type: "HASH".to_string(),
            },],
            global_secondary_indexes: Some(vec![GlobalSecondaryIndex {
                index_name: "pages_cbui_ca-index".to_string(),
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
                ..Default::default()
            },]),
            ..Default::default()
        },
        CreateTableInput {
            table_name: "page_user_sharing_permissions".to_string(),
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
                    attribute_name: "user_id".to_string(),
                    attribute_type: "S".to_string(),
                },
                AttributeDefinition {
                    attribute_name: "sharing_permission".to_string(),
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
                    attribute_name: "page_id".to_string(),
                    key_type: "HASH".to_string(),
                },
                KeySchemaElement {
                    attribute_name: "user_id".to_string(),
                    key_type: "RANGE".to_string(),
                }
            ],
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
