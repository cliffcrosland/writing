use actix_session::Session;
use actix_web::dev::HttpResponseBuilder;
use actix_web::http::{header, StatusCode};
use actix_web::{error, get, post, web, HttpResponse};
use askama::Template;
use rusoto_core::RusotoError;
use rusoto_dynamodb::{
    AttributeValue, DynamoDb, GetItemInput, PutItemError, PutItemInput, QueryInput, UpdateItemInput,
};
use serde::{Deserialize, Serialize};

use crate::dynamodb::{av_get_s, av_map, av_s, table_name};
use crate::http;
use crate::ids::{Id, IdType};
use crate::users::UserRole;
use crate::utils;
use crate::utils::time;
use crate::BackendService;

const INTERNAL_SERVER_ERROR_MESSAGE: &str = "Sorry, an error occurred. Please try again later.";
const USER_ALREADY_EXISTS_MESSAGE: &str =
    "This user already exists. Please try logging in instead.";

#[derive(Deserialize, Serialize)]
pub struct LoginForm {
    email: String,
    password: String,
}

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    email: String,
    password: String,
    error_message: String,
}

#[derive(Deserialize, Serialize)]
pub struct SignUpForm {
    email: String,
    password: String,
    password_confirmation: String,
}

#[derive(Template)]
#[template(path = "sign_up.html")]
struct SignUpTemplate {
    email: String,
    password: String,
    password_confirmation: String,
    error_message: String,
}

#[get("/log_in")]
pub async fn get_log_in(
    session: Session,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let user = http::get_session_user(&session, &service).await;
    if user.is_ok() {
        return Ok(HttpResponse::SeeOther()
            .header(header::LOCATION, "/app")
            .finish());
    }

    let body = LoginTemplate {
        email: String::new(),
        password: String::new(),
        error_message: String::new(),
    }
    .render()
    .unwrap();
    Ok(HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body))
}

#[get("/sign_up")]
pub async fn get_sign_up(
    session: Session,
    service: web::Data<BackendService>,
) -> actix_web::Result<HttpResponse> {
    let user = http::get_session_user(&session, &service).await;
    if user.is_ok() {
        return Ok(HttpResponse::SeeOther()
            .header(header::LOCATION, "/app")
            .finish());
    }
    let body = SignUpTemplate {
        email: String::new(),
        password: String::new(),
        password_confirmation: String::new(),
        error_message: String::new(),
    }
    .render()
    .unwrap();
    Ok(HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(body))
}

#[post("/log_in")]
pub async fn submit_log_in(
    session: Session,
    service: web::Data<BackendService>,
    form: web::Form<LoginForm>,
) -> actix_web::Result<HttpResponse> {
    let error_response = |status_code: StatusCode| -> HttpResponse {
        let error_message = match status_code {
            StatusCode::NOT_FOUND => "User was not found, or password was incorrect.",
            _ => INTERNAL_SERVER_ERROR_MESSAGE,
        };
        let body = LoginTemplate {
            email: form.email.clone(),
            password: form.password.clone(),
            error_message: String::from(error_message),
        }
        .render()
        .unwrap();
        HttpResponseBuilder::new(status_code)
            .content_type("text/html; charset=utf-8")
            .body(body)
    };

    if form.email.is_empty() || form.password.is_empty() {
        return Ok(error_response(StatusCode::NOT_FOUND));
    }

    let output = service
        .dynamodb_client
        .get_item(GetItemInput {
            table_name: table_name("users"),
            key: av_map(&[av_s("email", &form.email)]),
            projection_expression: Some("id, hashed_password".to_string()),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    if output.item.is_none() {
        return Ok(error_response(StatusCode::NOT_FOUND));
    }
    let item = output.item.unwrap();
    let user_id = av_get_s(&item, "id").ok_or_else(|| error::ErrorNotFound(""))?;
    let hashed_password =
        av_get_s(&item, "hashed_password").ok_or_else(|| error::ErrorNotFound(""))?;

    // Check to see if password matches
    let password_matched = bcrypt::verify(&form.password, &hashed_password).map_err(|e| {
        log::error!("{}", e);
        error_response(StatusCode::INTERNAL_SERVER_ERROR)
    })?;
    if !password_matched {
        return Ok(error_response(StatusCode::NOT_FOUND));
    }

    // Find the most recent org login for this user. Use that org for login.
    let output = service
        .dynamodb_client
        .query(QueryInput {
            table_name: table_name("organization_users"),
            // Scan the [user_id, last_login_at] index from most recent login to least. Take the
            // first result we find.
            index_name: Some("organization_users_user_id_last_login_at-index".to_string()),
            scan_index_forward: Some(false),
            limit: Some(1),
            key_condition_expression: Some("user_id = :user_id".to_string()),
            expression_attribute_values: Some(av_map(&[av_s(":user_id", user_id)])),
            projection_expression: Some("org_id".to_string()),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR)
        })?;
    if output.items.is_none() {
        return Ok(error_response(StatusCode::NOT_FOUND));
    }
    let items = output.items.unwrap();
    if items.len() != 1 {
        return Ok(error_response(StatusCode::NOT_FOUND));
    }
    let item = &items[0];
    let org_id = av_get_s(&item, "org_id").ok_or_else(|| error_response(StatusCode::NOT_FOUND))?;

    // Update last_login_at value to be "now"
    let now = chrono::Utc::now();
    service
        .dynamodb_client
        .update_item(UpdateItemInput {
            table_name: table_name("organization_users"),
            key: av_map(&[av_s("org_id", org_id), av_s("user_id", user_id)]),
            update_expression: Some("SET last_login_at = :now, updated_at = :now".to_string()),
            expression_attribute_values: Some(av_map(&[av_s(
                ":now",
                &utils::time::date_time_iso_str(&now),
            )])),
            ..Default::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error_response(StatusCode::INTERNAL_SERVER_ERROR)
        })?;

    // Store org_id and user_id in session cookie. A user who belongs to multiple orgs may switch
    // the org, which will update org_id in their session.
    session
        .set("org_id", org_id)
        .map_err(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR))?;
    session
        .set("user_id", user_id)
        .map_err(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR))?;

    // We use "303 See Other" redirect so that refreshing the destination page does not re-submit
    // the form via POST.
    //
    // See: https://developer.mozilla.org/en-US/docs/Web/HTTP/Redirections#temporary_redirections
    Ok(HttpResponse::SeeOther()
        .set_header(header::LOCATION, "/app")
        .finish())
}

#[post("/sign_up")]
pub async fn submit_sign_up(
    session: Session,
    service: web::Data<BackendService>,
    form: web::Form<SignUpForm>,
) -> actix_web::Result<HttpResponse> {
    let error_response = |status_code: StatusCode, error_message: &str| -> HttpResponse {
        let body = SignUpTemplate {
            email: form.email.clone(),
            password: form.password.clone(),
            password_confirmation: form.password_confirmation.clone(),
            error_message: String::from(error_message),
        }
        .render()
        .unwrap();
        HttpResponseBuilder::new(status_code)
            .content_type("text/html; charset=utf-8")
            .body(body)
    };

    if let Err(error_message) = validate_sign_up_form(&form) {
        return Ok(error_response(StatusCode::BAD_REQUEST, &error_message));
    }

    let output = service
        .dynamodb_client
        .get_item(GetItemInput {
            table_name: table_name("users"),
            consistent_read: Some(true),
            key: maplit::hashmap! {
                "email".to_string() => AttributeValue {
                    s: Some(form.email.clone()),
                    ..AttributeValue::default()
                }
            },
            projection_expression: Some("id".to_string()),
            ..GetItemInput::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL_SERVER_ERROR_MESSAGE,
            )
        })?;
    if output.item.is_some() {
        return Ok(error_response(
            StatusCode::BAD_REQUEST,
            USER_ALREADY_EXISTS_MESSAGE,
        ));
    }

    // Create user.
    //
    // TODO(cliff): Introduce a new flow to set fields of user profile.
    let user_id = Id::new(IdType::User);
    let hashed_password = bcrypt::hash(&form.password, bcrypt::DEFAULT_COST).map_err(|e| {
        log::error!("{}", e);
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL_SERVER_ERROR_MESSAGE,
        )
    })?;
    let now = time::date_time_iso_str(&chrono::Utc::now());
    service
        .dynamodb_client
        .put_item(PutItemInput {
            table_name: table_name("users"),
            // Preventing data race: Only overwrite if the user does not already exist.
            condition_expression: Some(
                "attribute_not_exists(id) and attribute_not_exists(email)".to_string(),
            ),
            item: maplit::hashmap! {
                "id".to_string() => AttributeValue {
                    s: Some(user_id.as_str().to_string()),
                    ..AttributeValue::default()
                },
                "email".to_string() => AttributeValue {
                    s: Some(form.email.clone()),
                    ..AttributeValue::default()
                },
                "name".to_string() => AttributeValue {
                    s: Some(form.email.clone()),
                    ..AttributeValue::default()
                },
                "hashed_password".to_string() => AttributeValue {
                    s: Some(hashed_password),
                    ..AttributeValue::default()
                },
                "photo_url".to_string() => AttributeValue {
                    null: Some(true),
                    ..AttributeValue::default()
                },
                "created_at".to_string() => AttributeValue {
                    s: Some(now.clone()),
                    ..AttributeValue::default()
                },
                "updated_at".to_string() => AttributeValue {
                    s: Some(now.clone()),
                    ..AttributeValue::default()
                },
            },
            ..PutItemInput::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            match e {
                RusotoError::Service(PutItemError::ConditionalCheckFailed(_)) => {
                    error_response(StatusCode::BAD_REQUEST, USER_ALREADY_EXISTS_MESSAGE)
                }
                _ => error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    INTERNAL_SERVER_ERROR_MESSAGE,
                ),
            }
        })?;

    // Create organization, and add user to the organization.
    //
    // TODO(cliff): Introduce a new flow to create organizations. For now, just create an arbitrary
    // organization with a single user whenever a user signs up.
    let org_id = Id::new(IdType::Organization);
    let org_name = format!("Organization created by {}", &form.email);
    service
        .dynamodb_client
        .put_item(PutItemInput {
            table_name: table_name("organizations"),
            condition_expression: Some("attribute_not_exists(id)".to_string()),
            item: maplit::hashmap! {
                "id".to_string() => AttributeValue {
                    s: Some(org_id.as_str().to_string()),
                    ..AttributeValue::default()
                },
                "name".to_string() => AttributeValue {
                    s: Some(org_name.clone()),
                    ..AttributeValue::default()
                },
                "logo_url".to_string() => AttributeValue {
                    null: Some(true),
                    ..AttributeValue::default()
                },
                "created_at".to_string() => AttributeValue {
                    s: Some(now.clone()),
                    ..AttributeValue::default()
                },
                "updated_at".to_string() => AttributeValue {
                    s: Some(now.clone()),
                    ..AttributeValue::default()
                },
            },
            ..PutItemInput::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL_SERVER_ERROR_MESSAGE,
            )
        })?;
    service
        .dynamodb_client
        .put_item(PutItemInput {
            table_name: table_name("organization_users"),
            item: maplit::hashmap! {
                "org_id".to_string() => AttributeValue {
                    s: Some(org_id.as_str().to_string()),
                    ..AttributeValue::default()
                },
                "user_id".to_string() => AttributeValue {
                    s: Some(user_id.as_str().to_string()),
                    ..AttributeValue::default()
                },
                "last_login_at".to_string() => AttributeValue {
                    s: Some(now.clone()),
                    ..AttributeValue::default()
                },
                "user_role".to_string() => AttributeValue {
                    n: Some((UserRole::OrgAdmin as i32).to_string()),
                    ..AttributeValue::default()
                },
                "created_at".to_string() => AttributeValue {
                    s: Some(now.clone()),
                    ..AttributeValue::default()
                },
                "updated_at".to_string() => AttributeValue {
                    s: Some(now.clone()),
                    ..AttributeValue::default()
                },
            },
            ..PutItemInput::default()
        })
        .await
        .map_err(|e| {
            log::error!("{}", e);
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                INTERNAL_SERVER_ERROR_MESSAGE,
            )
        })?;

    session.set("org_id", org_id.as_str()).map_err(|_| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL_SERVER_ERROR_MESSAGE,
        )
    })?;
    session.set("user_id", user_id.as_str()).map_err(|_| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            INTERNAL_SERVER_ERROR_MESSAGE,
        )
    })?;

    // We use "303 See Other" redirect so that refreshing the destination page does not re-submit
    // the form via POST.
    //
    // See: https://developer.mozilla.org/en-US/docs/Web/HTTP/Redirections#temporary_redirections
    Ok(HttpResponse::SeeOther()
        .set_header(header::LOCATION, "/app")
        .finish())
}

#[post("/log_out")]
pub async fn submit_log_out(session: Session) -> actix_web::Result<HttpResponse> {
    session.purge();
    Ok(HttpResponse::SeeOther()
        .set_header(header::LOCATION, "/log_in")
        .finish())
}

fn validate_sign_up_form(form: &SignUpForm) -> Result<(), String> {
    if form.email.is_empty() || form.password.is_empty() || form.password_confirmation.is_empty() {
        return Err("Email, password, and confirmed password cannot be empty.".into());
    }
    if !form.email.contains('@') {
        return Err("Must be a valid email address.".into());
    }
    if form.password != form.password_confirmation {
        return Err("Password and confirmed password must match.".into());
    }
    if form.password.len() < 10 {
        return Err("Password must contain at least 10 characters.".into());
    }
    let lower_case = regex::Regex::new(r"[a-z]").unwrap();
    let upper_case = regex::Regex::new(r"[A-Z]").unwrap();
    let number = regex::Regex::new(r"[0-9]").unwrap();
    let special_character = regex::Regex::new(r"[~!@#$%^*\-_=+\[{\]}/;:,.?]").unwrap();
    if !lower_case.is_match(&form.password) {
        return Err("Password must contain at least one lower case letter.".into());
    }
    if !upper_case.is_match(&form.password) {
        return Err("Password must contain at least one upper case letter.".into());
    }
    if !number.is_match(&form.password) {
        return Err("Password must contain at least one number.".into());
    }
    if !special_character.is_match(&form.password) {
        return Err("Password must contain at least one special character: ~ ! @ # $ % ^ * - _ = + [ { ] } / ; : , . ?".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use actix_web::http::StatusCode;
    use actix_web::test;
    use actix_web::test::TestRequest;
    use actix_web::App;
    use chrono::Utc;
    use cookie::Cookie;

    use crate::ids::{Id, IdType};
    use crate::testing::utils::{
        decrypt_session_cookie_value, default_backend_service, default_cookie_session, TestDynamoDb,
    };

    use crate::testing::fixtures::{create_organization_user, create_user};

    #[tokio::test]
    async fn test_login_success() {
        let db = TestDynamoDb::new().await;

        // Create user, add to org
        let org_id = Id::new(IdType::Organization);
        let user_id = create_user(&db.dynamodb_client, "jane@smith.com", "Jane Smith").await;
        let last_login_at = Utc::now() - chrono::Duration::days(1);
        create_organization_user(&db.dynamodb_client, &org_id, &user_id, &last_login_at).await;

        // Set hashed password
        let password = "KDIo*kJDLJ(1j1;;asdf;1;;1testtesttest";
        let hashed_password = bcrypt::hash(password, 4).unwrap();
        db.dynamodb_client
            .update_item(UpdateItemInput {
                table_name: table_name("users"),
                key: av_map(&[av_s("email", "jane@smith.com")]),
                update_expression: Some("SET hashed_password = :hashed_password".to_string()),
                expression_attribute_values: Some(av_map(&[av_s(
                    ":hashed_password",
                    &hashed_password,
                )])),
                ..Default::default()
            })
            .await
            .unwrap();

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_cookie_session())
                .service(submit_log_in),
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

        assert_eq!(user_id.as_str(), &session_user_id);
        assert_eq!(org_id.as_str(), &session_org_id);
    }

    #[tokio::test]
    async fn test_login_user_not_found() {
        let db = TestDynamoDb::new().await;

        let org_id = Id::new(IdType::Organization);
        let user_id = create_user(&db.dynamodb_client, "jane@smith.com", "Jane Smith").await;
        let last_login_at = Utc::now() - chrono::Duration::days(1);
        create_organization_user(&db.dynamodb_client, &org_id, &user_id, &last_login_at).await;

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_cookie_session())
                .service(submit_log_in),
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
        let cookies: Vec<Cookie> = response.response().cookies().collect();
        assert_eq!(cookies.len(), 1);
        let encrypted_cookie = cookies[0].clone().into_owned();
        let decrypted_cookie_value = decrypt_session_cookie_value(&encrypted_cookie, "session");
        assert!(decrypted_cookie_value.is_some());
        let decrypted_cookie_value = decrypted_cookie_value.unwrap();
        let session_map: HashMap<String, String> =
            serde_json::from_str(&decrypted_cookie_value).unwrap();
        assert!(session_map.is_empty());
    }

    #[tokio::test]
    async fn test_login_organization_user_not_found() {
        let db = TestDynamoDb::new().await;

        // Create user, but do not add them to an org.
        create_user(&db.dynamodb_client, "jane@smith.com", "Jane Smith").await;

        // Set hashed password
        let password = "KDIo*kJDLJ(1j1;;asdf;1;;1testtesttest";
        let hashed_password = bcrypt::hash(password, 4).unwrap();
        db.dynamodb_client
            .update_item(UpdateItemInput {
                table_name: table_name("users"),
                key: av_map(&[av_s("email", "jane@smith.com")]),
                update_expression: Some("SET hashed_password = :hashed_password".to_string()),
                expression_attribute_values: Some(av_map(&[av_s(
                    ":hashed_password",
                    &hashed_password,
                )])),
                ..Default::default()
            })
            .await
            .unwrap();

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_cookie_session())
                .service(submit_log_in),
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
        let cookies: Vec<Cookie> = response.response().cookies().collect();
        assert_eq!(cookies.len(), 1);
        let encrypted_cookie = cookies[0].clone().into_owned();
        let decrypted_cookie_value = decrypt_session_cookie_value(&encrypted_cookie, "session");
        assert!(decrypted_cookie_value.is_some());
        let decrypted_cookie_value = decrypted_cookie_value.unwrap();
        let session_map: HashMap<String, String> =
            serde_json::from_str(&decrypted_cookie_value).unwrap();
        assert!(session_map.is_empty());
    }

    #[test]
    fn test_sign_up_form_validation() {
        let create_form =
            |email: &str, password: &str, password_confirmation: &str| -> SignUpForm {
                SignUpForm {
                    email: email.to_string(),
                    password: password.to_string(),
                    password_confirmation: password_confirmation.to_string(),
                }
            };
        let valid_password = "US!019hdja00;tQ9102u3";

        let result = validate_sign_up_form(&create_form("", "", ""));
        assert_eq!(
            result.err().unwrap(),
            "Email, password, and confirmed password cannot be empty."
        );
        let result = validate_sign_up_form(&create_form("foo", valid_password, valid_password));
        assert_eq!(result.err().unwrap(), "Must be a valid email address.");
        let result =
            validate_sign_up_form(&create_form("foo@bar.com", valid_password, "non matching"));
        assert_eq!(
            result.err().unwrap(),
            "Password and confirmed password must match."
        );
        let result = validate_sign_up_form(&create_form("foo@bar.com", "Abc123!", "Abc123!"));
        assert_eq!(
            result.err().unwrap(),
            "Password must contain at least 10 characters."
        );
        let result = validate_sign_up_form(&create_form("foo@bar.com", "ABC123456!", "ABC123456!"));
        assert_eq!(
            result.err().unwrap(),
            "Password must contain at least one lower case letter."
        );
        let result = validate_sign_up_form(&create_form("foo@bar.com", "abc123456!", "abc123456!"));
        assert_eq!(
            result.err().unwrap(),
            "Password must contain at least one upper case letter."
        );
        let result =
            validate_sign_up_form(&create_form("foo@bar.com", "ABCabcdefg!", "ABCabcdefg!"));
        assert_eq!(
            result.err().unwrap(),
            "Password must contain at least one number."
        );
        let result =
            validate_sign_up_form(&create_form("foo@bar.com", "ABCabc12345", "ABCabc12345"));
        assert_eq!(result.err().unwrap(), "Password must contain at least one special character: ~ ! @ # $ % ^ * - _ = + [ { ] } / ; : , . ?");
        let result =
            validate_sign_up_form(&create_form("foo@bar.com", valid_password, valid_password));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_sign_up_success() {
        // NOTE: This test runs a little slowly, about 1 second, because it uses `bcrypt::hash` to
        // generate a secure hashed password when a user signs up.
        let db = TestDynamoDb::new().await;

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_cookie_session())
                .service(submit_sign_up),
        )
        .await;

        let form = SignUpForm {
            email: "jane@smith.com".to_string(),
            password: "AJLK:jasd;lj123123".to_string(),
            password_confirmation: "AJLK:jasd;lj123123".to_string(),
        };
        let request = TestRequest::post()
            .uri("/sign_up")
            .header("content-type", "application/x-www-form-urlencoded")
            .set_form(&form)
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

        let output = db
            .dynamodb_client
            .get_item(GetItemInput {
                table_name: table_name("users"),
                key: maplit::hashmap! {
                    "email".to_string() => AttributeValue {
                        s: Some(form.email.clone()),
                        ..AttributeValue::default()
                    }
                },
                ..GetItemInput::default()
            })
            .await
            .unwrap();
        assert!(output.item.is_some());
        let user = output.item.unwrap();
        assert_eq!(av_get_s(&user, "email").unwrap(), form.email);
        let hashed_password = av_get_s(&user, "hashed_password").unwrap();
        assert!(bcrypt::verify(&form.password, hashed_password).unwrap());
        let user_id = av_get_s(&user, "id").unwrap();
        assert_eq!(session_user_id, user_id);

        let output = db
            .dynamodb_client
            .get_item(GetItemInput {
                table_name: table_name("organization_users"),
                key: maplit::hashmap! {
                    "org_id".to_string() => AttributeValue {
                        s: Some(session_org_id.as_str().to_string()),
                        ..AttributeValue::default()
                    },
                    "user_id".to_string() => AttributeValue {
                        s: Some(session_user_id.as_str().to_string()),
                        ..AttributeValue::default()
                    },
                },
                projection_expression: Some("last_login_at".to_string()),
                ..GetItemInput::default()
            })
            .await
            .unwrap();
        let organization_user = output.item.unwrap();
        let last_login_at = av_get_s(&organization_user, "last_login_at").unwrap();
        let last_login_date_time = chrono::DateTime::parse_from_rfc3339(&last_login_at).unwrap();
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(last_login_date_time);
        assert!(duration < chrono::Duration::seconds(10));

        let output = db
            .dynamodb_client
            .get_item(GetItemInput {
                table_name: table_name("organizations"),
                key: maplit::hashmap! {
                    "id".to_string() => AttributeValue {
                        s: Some(session_org_id.clone()),
                        ..AttributeValue::default()
                    }
                },
                ..GetItemInput::default()
            })
            .await
            .unwrap();
        let org = output.item.unwrap();
        let org_id = av_get_s(&org, "id").unwrap();
        assert_eq!(session_org_id, org_id);
        let org_name = av_get_s(&org, "name").unwrap();
        assert_eq!(format!("Organization created by {}", &form.email), org_name);
    }

    #[tokio::test]
    async fn test_sign_up_user_already_exists() {
        let db = TestDynamoDb::new().await;

        create_user(&db.dynamodb_client, "jane@smith.com", "Jane Smith").await;

        let mut test_app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_cookie_session())
                .service(submit_sign_up),
        )
        .await;

        let form = SignUpForm {
            email: "jane@smith.com".to_string(),
            password: "AJLK:jasd;lj123123".to_string(),
            password_confirmation: "AJLK:jasd;lj123123".to_string(),
        };
        let request = TestRequest::post()
            .uri("/sign_up")
            .header("content-type", "application/x-www-form-urlencoded")
            .set_form(&form)
            .to_request();

        let response = test::call_service(&mut test_app, request).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let cookies: Vec<Cookie> = response.response().cookies().collect();
        assert_eq!(cookies.len(), 1);
        let encrypted_cookie = cookies[0].clone().into_owned();
        let decrypted_cookie_value = decrypt_session_cookie_value(&encrypted_cookie, "session");
        assert!(decrypted_cookie_value.is_some());
        let decrypted_cookie_value = decrypted_cookie_value.unwrap();
        let session_map: HashMap<String, String> =
            serde_json::from_str(&decrypted_cookie_value).unwrap();
        assert!(session_map.is_empty());
    }
}
