#[cfg(test)]
use chrono::{DateTime, Utc};

use rusoto_dynamodb::{DynamoDb, PutItemInput};
use uuid::Uuid;

use crate::dynamodb::{av_map, av_n, av_s, table_name};
use crate::utils;

pub async fn create_user(dynamodb_client: &dyn DynamoDb, email: &str, name: &str) -> Uuid {
    let user_id = Uuid::new_v4();
    let now_str = utils::time::date_time_iso_str(&Utc::now());

    dynamodb_client
        .put_item(PutItemInput {
            table_name: table_name("users"),
            item: av_map(&[
                av_s("email", email),
                av_s("name", name),
                av_s("id", &user_id.to_hyphenated().to_string()),
                av_s("hashed_password", ""),
                av_s("photo_url", ""),
                av_s("created_at", &now_str),
                av_s("updated_at", &now_str),
            ]),
            ..Default::default()
        })
        .await
        .unwrap();
    user_id
}

pub async fn create_organization_user(
    dynamodb_client: &dyn DynamoDb,
    org_id: &Uuid,
    user_id: &Uuid,
    last_login_at: &DateTime<Utc>,
) {
    let now_str = utils::time::date_time_iso_str(&Utc::now());
    dynamodb_client
        .put_item(PutItemInput {
            table_name: table_name("organization_users"),
            item: av_map(&[
                av_s("org_id", &org_id.to_hyphenated().to_string()),
                av_s("user_id", &user_id.to_hyphenated().to_string()),
                av_n("role", 0_i32.to_string()),
                av_s(
                    "last_login_at",
                    &utils::time::date_time_iso_str(last_login_at),
                ),
                av_s("created_at", &now_str),
                av_s("updated_at", &now_str),
            ]),
            ..Default::default()
        })
        .await
        .unwrap();
}
