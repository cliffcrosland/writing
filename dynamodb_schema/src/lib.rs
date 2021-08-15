/// Schema for the DynamoDB tables.
///
/// NOTE: When you are choosing attribute names, avoid using DynamoDB Reserved Words unless you
/// have no other choice. Query ergonomics get bad otherwise. For example, instead of using "role"
/// as an attribute, use "user_role" since it is not a reserved word.
///
/// Here is the list of DynamoDB Reserved Words:
/// https://docs.aws.amazon.com/amazondynamodb/latest/developerguide/ReservedWords.html

use lazy_static::lazy_static;
use rusoto_dynamodb::{
    AttributeDefinition, CreateTableInput, GlobalSecondaryIndex, KeySchemaElement, Projection,
    ProvisionedThroughput,
};

lazy_static! {
    pub static ref TABLE_DEFINITIONS: Vec<CreateTableInput> = vec![
        CreateTableInput {
            table_name: "users".to_string(),
            /*
             * users
             *
             *   id: string, u_<id>
             *   email: string
             *   name: string
             *   hashed_password: string
             *   photo_url: string
             *   created_at: string, iso 8601 date time
             *   updated_at: string, iso 8601 date time
             *
             * primary key:
             *
             *   [email]
             *
             * global secondary indexes:
             *
             *   [id]
             */
            attribute_definitions: vec![
                attr_def("id", "S"),
                attr_def("email", "S"),
            ],
            key_schema: vec![key_schema_elem("email", "HASH"),],
            global_secondary_indexes: Some(vec![GlobalSecondaryIndex {
                index_name: "id-index".to_string(),
                key_schema: vec![KeySchemaElement {
                    attribute_name: "id".to_string(),
                    key_type: "HASH".to_string(),
                }],
                projection: Projection {
                    projection_type: Some("ALL".to_string()),
                    ..Default::default()
                },
                provisioned_throughput: default_provisioned_throughput(),
            }]),
            provisioned_throughput: default_provisioned_throughput(),
            ..Default::default()
        },
        CreateTableInput {
            /*
             * organizations
             *
             *   id: string, o_<id>
             *   name: string
             *   logo_url: string
             *   created_at: string, iso 8601 date time
             *   updated_at: string, iso 8601 date time
             *
             * primary key:
             *
             *   [id]
             *
             */
            table_name: "organizations".to_string(),
            attribute_definitions: vec![
                attr_def("id", "S"),
            ],
            key_schema: vec![key_schema_elem("id", "HASH")],
            provisioned_throughput: default_provisioned_throughput(),
            ..Default::default()
        },
        CreateTableInput {
            /*
             * organization_users
             *
             *   org_id: string, o_<id>
             *   user_id: string, u_<id>
             *   last_login_at: string, iso 8601 date time
             *   user_role: int
             *   created_at: string, iso 8601 date time
             *   updated_at: string, iso 8601 date time
             *
             * primary key:
             *
             *   [org_id, user_id]
             *
             * global secondary indexes:
             *
             *   [user_id, last_login_at]
             */
            table_name: "organization_users".to_string(),
            attribute_definitions: vec![
                attr_def("org_id", "S"),
                attr_def("user_id", "S"),
                attr_def("last_login_at", "S"),
            ],
            key_schema: vec![
                key_schema_elem("org_id", "HASH"),
                key_schema_elem("user_id", "RANGE"),
            ],
            global_secondary_indexes: Some(vec![GlobalSecondaryIndex {
                index_name: "user_id-last_login_at-index".to_string(),
                key_schema: vec![
                    key_schema_elem("user_id", "HASH"),
                    key_schema_elem("last_login_at", "RANGE"),
                ],
                projection: Projection {
                    projection_type: Some("ALL".to_string()),
                    ..Default::default()
                },
                provisioned_throughput: default_provisioned_throughput(),
            }]),
            provisioned_throughput: default_provisioned_throughput(),
            ..Default::default()
        },
        CreateTableInput {
            /*
             * documents
             *
             *   id: string, d_<id>
             *   org_id: string, o_<id>
             *   title: string
             *   created_by_user_id: string, u_<id>
             *   org_level_sharing_permission: int, enum
             *   created_at: string, iso 8601 date time
             *   updated_at: string, iso 8601 date time
             *
             * primary key:
             *
             *   [id]
             *
             */
            table_name: "documents".to_string(),
            attribute_definitions: vec![
                attr_def("id", "S"),
                attr_def("created_by_user_id", "S"),
                attr_def("updated_at", "S"),
            ],
            key_schema: vec![key_schema_elem("id", "HASH"),],
            global_secondary_indexes: Some(vec![GlobalSecondaryIndex {
                index_name: "created_by_user_id-updated_at-index".to_string(),
                key_schema: vec![
                    key_schema_elem("created_by_user_id", "HASH"),
                    key_schema_elem("updated_at", "RANGE"),
                ],
                projection: Projection {
                    projection_type: Some("ALL".to_string()),
                    ..Default::default()
                },
                provisioned_throughput: default_provisioned_throughput(),
                ..Default::default()
            }]),
            provisioned_throughput: default_provisioned_throughput(),
            ..Default::default()
        },
        CreateTableInput {
            /*
             * document_user_sharing_permissions
             *
             *   doc_id: string, d_<id>
             *   user_id: string, u_<id>
             *   org_id: string, o_<id>
             *   sharing_permission: int, enum
             *   created_at: string, iso 8601 date time
             *   updated_at: string, iso 8601 date time
             *
             * primary key:
             *
             *   [doc_id, user_id]
             */
            table_name: "document_user_sharing_permissions".to_string(),
            attribute_definitions: vec![
                attr_def("doc_id", "S"),
                attr_def("user_id", "S"),
            ],
            key_schema: vec![
                key_schema_elem("doc_id", "HASH"),
                key_schema_elem("user_id", "RANGE"),
            ],
            provisioned_throughput: default_provisioned_throughput(),
            ..Default::default()
        },
        CreateTableInput {
            /*
             * document_revisions
             *
             *   doc_id: string, d_<id>
             *   author_user_id: string, u_<id>
             *   revision_number: integer
             *   change_set: binary, protobuf message
             *   committed_at: string, iso 8601 date time
             *
             * primary key:
             *
             *   [doc_id, revision]
             */

            table_name: "document_revisions".to_string(),
            attribute_definitions: vec![
                attr_def("doc_id", "S"),
                attr_def("revision_number", "N"),
            ],
            key_schema: vec![
                key_schema_elem("doc_id", "HASH"),
                key_schema_elem("revision_number", "RANGE"),
            ],
            provisioned_throughput: default_provisioned_throughput(),
            ..Default::default()
        },
    ];
}

fn attr_def(attribute_name: &str, attribute_type: &str) -> AttributeDefinition {
    AttributeDefinition {
        attribute_name: attribute_name.to_string(),
        attribute_type: attribute_type.to_string(),
    }
}

fn key_schema_elem(attribute_name: &str, key_type: &str) -> KeySchemaElement {
    KeySchemaElement {
        attribute_name: attribute_name.to_string(),
        key_type: key_type.to_string(),
    }
}

fn default_provisioned_throughput() -> Option<ProvisionedThroughput> {
    Some(ProvisionedThroughput {
        read_capacity_units: 100,
        write_capacity_units: 100,
    })
}
