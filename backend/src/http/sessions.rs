use actix_session::Session;
use actix_web::http::header;
use actix_web::{error, post, web, HttpResponse};
use rusoto_dynamodb::{AttributeValue, DynamoDb, QueryInput, UpdateItemInput};
use serde::{Deserialize, Serialize};

use crate::dynamodb::dynamodb_table_name;
use crate::utils;
use crate::utils::ToAttributeValueMap;
use crate::BackendService;

#[derive(Deserialize, Serialize)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
}

#[post("/log_in")]
pub async fn log_in(
    session: Session,
    form: web::Form<LoginForm>,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let result = service
        .dynamodb_client
        .query(QueryInput {
            table_name: dynamodb_table_name("users"),
            key_condition_expression: Some("email = :email".to_string()),
            expression_attribute_values: Some(
                [(
                    ":email".to_string(),
                    AttributeValue {
                        s: Some(form.email.clone()),
                        ..Default::default()
                    },
                )]
                .to_attribute_value_map(),
            ),
            attributes_to_get: Some(vec!["id".to_string(), "hashed_password".to_string()]),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error::ErrorInternalServerError("")
        })?;

    if result.items.is_none() {
        return Err(error::ErrorNotFound(""));
    }
    let items = result.items.unwrap();
    if items.len() != 1 {
        return Err(error::ErrorNotFound(""));
    }
    let item = &items[0];
    let user_id = item
        .get("user_id")
        .ok_or_else(|| error::ErrorNotFound(""))?
        .s
        .as_ref()
        .ok_or_else(|| error::ErrorNotFound(""))?;
    let hashed_password = item
        .get("hashed_password")
        .ok_or_else(|| error::ErrorNotFound(""))?
        .s
        .as_ref()
        .ok_or_else(|| error::ErrorNotFound(""))?;

    // Check to see if password matches
    let password_matched = bcrypt::verify(&form.password, &hashed_password).map_err(|e| {
        log::error!("{}", e);
        error::ErrorInternalServerError("")
    })?;
    if !password_matched {
        return Ok(HttpResponse::NotFound().finish());
    }

    // Find the most recent org login for this user. Use that org for login.
    let result = service
        .dynamodb_client
        .query(QueryInput {
            table_name: dynamodb_table_name("organization_users"),
            // Scan the [user_id, last_login_at] index from most recent login to least. Take the
            // first result we find.
            index_name: Some("organization_users_user_id_last_login_at-index".to_string()),
            scan_index_forward: Some(false),
            limit: Some(1),
            key_condition_expression: Some("user_id = :user_id".to_string()),
            expression_attribute_values: Some(
                [(
                    ":user_id".to_string(),
                    AttributeValue {
                        s: Some(user_id.clone()),
                        ..Default::default()
                    },
                )]
                .to_attribute_value_map(),
            ),
            attributes_to_get: Some(vec!["org_id".to_string()]),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error::ErrorInternalServerError("")
        })?;
    if result.items.is_none() {
        return Err(error::ErrorNotFound(""));
    }
    let items = result.items.unwrap();
    if items.len() != 1 {
        return Err(error::ErrorNotFound(""));
    }
    let item = &items[0];
    let org_id = item
        .get("org_id")
        .ok_or_else(|| error::ErrorNotFound(""))?
        .s
        .as_ref()
        .ok_or_else(|| error::ErrorNotFound(""))?;

    // Update last_login_at value to be "now", num milliseconds since unix epoch.
    let now = utils::current_time_millis();
    service
        .dynamodb_client
        .update_item(UpdateItemInput {
            table_name: dynamodb_table_name("organization_users"),
            key: [
                (
                    "org_id".to_string(),
                    AttributeValue {
                        s: Some(org_id.to_string()),
                        ..Default::default()
                    },
                ),
                (
                    "user_id".to_string(),
                    AttributeValue {
                        s: Some(user_id.to_string()),
                        ..Default::default()
                    },
                ),
            ]
            .to_attribute_value_map(),
            update_expression: Some("SET last_login_at = :now, updated_at = :now".to_string()),
            expression_attribute_values: Some(
                [(
                    ":now".to_string(),
                    AttributeValue {
                        n: Some(now.to_string()),
                        ..Default::default()
                    },
                )]
                .to_attribute_value_map(),
            ),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error::ErrorInternalServerError("")
        })?;

    // Store org_id and user_id in session cookie. A user who belong to multiple orgs may switch
    // their org later, which will update org_id in their session.
    session
        .set("org_id", org_id)
        .map_err(|_| error::ErrorInternalServerError(""))?;
    session
        .set("user_id", user_id)
        .map_err(|_| error::ErrorInternalServerError(""))?;

    // We use "303 See Other" redirect so that refreshing the destination page does not re-submit
    // the log_in form via POST.
    //
    // See: https://developer.mozilla.org/en-US/docs/Web/HTTP/Redirections#temporary_redirections
    Ok(HttpResponse::SeeOther()
        .set_header(header::LOCATION, "/app") // TODO(cliff): Do we need full URL?
        .finish())
}

#[post("/log_out")]
pub async fn log_out(session: Session) -> actix_web::Result<HttpResponse> {
    session.purge();
    // TODO(cliff): Redirect to home?
    Ok(HttpResponse::Ok().finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use actix_web::http::StatusCode;
    use actix_web::test;
    use actix_web::test::TestRequest;
    use actix_web::App;
    use cookie::Cookie;
    use uuid::Uuid;

    use crate::testing::utils::{
        decrypt_session_cookie_value, default_backend_service, default_cookie_session, TestDynamoDb,
    };

    #[tokio::test]
    async fn test_login_success() {
        let db = TestDynamoDb::new().await;

        /*
        let pool = TestDbPool::new().await;

        let org_id = Uuid::new_v4();
        let user_id = create_user(pool.db_pool(), &org_id, "jane@smith.com", "Jane Smith").await;

        let password = "KDIo*kJDLJ(1j1;;asdf;1;;1testtesttest";
        let hashed_password = bcrypt::hash(password, 4).unwrap();
        sqlx::query("UPDATE users SET hashed_password = $1 WHERE id = $2")
            .bind(&hashed_password)
            .bind(&user_id)
            .execute(pool.db_pool())
            .await
            .unwrap();

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service(pool.db_pool_clone()).await)
                .wrap(default_cookie_session())
                .service(log_in),
        )
        .await;

        let login_form = LoginForm {
            email: "jane@smith.com".to_string(),
            password: password.to_string(),
        };
        let request = TestRequest::post()
            .uri("/log_in")
            .header("content-type", "application/x-www-form-urlencoded")
            .set_form(&login_form)
            .to_request();

        let response = test::call_service(&mut test_app, request).await;

        assert_eq!(response.status(), StatusCode::SEE_OTHER);

        let cookies: Vec<Cookie> = response.response().cookies().collect();
        assert_eq!(cookies.len(), 1);
        let encrypted_cookie = cookies[0].clone().into_owned();
        let decrypted_cookie_value = decrypt_session_cookie_value(&encrypted_cookie, "session");
        assert!(decrypted_cookie_value.is_some());
        let decrypted_cookie_value = decrypted_cookie_value.unwrap();

        let session_map: HashMap<String, String> =
            serde_json::from_str(&decrypted_cookie_value).unwrap();
        let session_user_id: String =
            serde_json::from_str(&session_map.get("user_id").unwrap()).unwrap();
        let session_org_id: String =
            serde_json::from_str(&session_map.get("org_id").unwrap()).unwrap();

        assert_eq!(user_id.to_simple().to_string(), session_user_id);
        assert_eq!(org_id.to_simple().to_string(), session_org_id);
        */
    }

    #[tokio::test]
    async fn test_login_user_not_found() {
        /*
        let pool = TestDbPool::new().await;

        let org_id = Uuid::new_v4();
        create_user(pool.db_pool(), &org_id, "jane@smith.com", "Jane Smith").await;

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service(pool.db_pool_clone()).await)
                .wrap(default_cookie_session())
                .service(log_in),
        )
        .await;

        let login_form = LoginForm {
            email: "some@randomemail.com".to_string(),
            password: "foobar123123!!!Foobar".to_string(),
        };
        let request = TestRequest::post()
            .uri("/log_in")
            .header("content-type", "application/x-www-form-urlencoded")
            .set_form(&login_form)
            .to_request();

        let response = test::call_service(&mut test_app, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        */
    }

    #[tokio::test]
    async fn test_login_organization_user_not_found() {
        /*
        let pool = TestDbPool::new().await;

        let org_id = Uuid::new_v4();
        let user_id = create_user(pool.db_pool(), &org_id, "jane@smith.com", "Jane Smith").await;

        // Set password for user
        let password = "KDIo*kJDLJ(1j1;;asdf;1;;1testtesttest";
        let hashed_password = bcrypt::hash(password, 4).unwrap();
        sqlx::query("UPDATE users SET hashed_password = $1 WHERE id = $2")
            .bind(&hashed_password)
            .bind(&user_id)
            .execute(pool.db_pool())
            .await
            .unwrap();

        // Remove the user from the org.
        sqlx::query("DELETE FROM organization_users")
            .execute(pool.db_pool())
            .await
            .unwrap();

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service(pool.db_pool_clone()).await)
                .wrap(default_cookie_session())
                .service(log_in),
        )
        .await;

        let login_form = LoginForm {
            email: "jane@smith.com".to_string(),
            password: "foobar123123!!!Foobar".to_string(),
        };
        let request = TestRequest::post()
            .uri("/log_in")
            .header("content-type", "application/x-www-form-urlencoded")
            .set_form(&login_form)
            .to_request();

        let response = test::call_service(&mut test_app, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        */
    }
}
