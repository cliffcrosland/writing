use std::sync::Arc;

use actix_identity::Identity;
use actix_web::{error, post, web, HttpResponse};
use prost::Message;
use sqlx::prelude::PgQueryAs;
use sqlx::types::chrono::NaiveDateTime;
use uuid::Uuid;

use crate::http;
use crate::proto::writing::{
    CreatePageRequest, CreatePageResponse, LoadPageRequest, LoadPageResponse, Page, PageNode,
};
use crate::utils;
use crate::BackendService;

#[post("/api/pages.createPage")]
pub async fn create_page(
    id: Identity,
    body: web::Bytes,
    service: web::Data<Arc<BackendService>>,
) -> actix_web::Result<HttpResponse> {
    let user = http::get_session_user(id, &service).await?;
    let request = CreatePageRequest::decode(body).map_err(|_| error::ErrorBadRequest(""))?;
    let (page_id, created_at): (Uuid, NaiveDateTime) = sqlx::query_as(
        "INSERT INTO pages \
        (org_id, title, created_by_user_id, last_edited_by_user_id, created_at, updated_at) \
        VALUES ($1, $2, $3, $3, now(), now()) \
        RETURNING id, created_at",
    )
    .bind(&user.org_id)
    .bind(&request.title)
    .bind(&user.id)
    .fetch_one(&service.db_pool)
    .await
    .map_err(|e| {
        log::error!("{}", e);
        error::ErrorInternalServerError("")
    })?;

    let created_at_micros = utils::date_time_to_micros(created_at);
    let response = CreatePageResponse {
        page: Some(Page {
            id: page_id.to_simple().to_string(),
            org_id: user.org_id.to_simple().to_string(),
            title: request.title,
            created_by_user_id: user.id.to_simple().to_string(),
            last_edited_by_user_id: user.id.to_simple().to_string(),
            project_owner_user_id: "".to_string(),
            created_at: created_at_micros,
            updated_at: created_at_micros,
        }),
    };
    http::create_protobuf_http_response(&response)
}

#[post("/api/pages.loadPage")]
async fn load_page(
    id: Identity,
    body: web::Bytes,
    service: web::Data<Arc<BackendService>>,
) -> actix_web::Result<HttpResponse> {
    let user = http::get_session_user(id, &service).await?;
    let request = LoadPageRequest::decode(body).map_err(|_| error::ErrorBadRequest(""))?;
    let request_org_id =
        Uuid::parse_str(&request.org_id).map_err(|_| error::ErrorBadRequest(""))?;
    if user.org_id != request_org_id {
        return Err(error::ErrorUnauthorized(""));
    }
    let request_page_id =
        Uuid::parse_str(&request.page_id).map_err(|_| error::ErrorBadRequest(""))?;

    #[derive(sqlx::FromRow)]
    struct PageFromRow {
        title: String,
        created_by_user_id: Uuid,
        last_edited_by_user_id: Uuid,
        project_owner_user_id: Option<Uuid>,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    }
    let page_meta_query = sqlx::query_as(
        "SELECT title, created_by_user_id, last_edited_by_user_id, \
                project_owner_user_id, created_at, updated_at \
         FROM pages \
         WHERE org_id = $1 AND id = $2",
    )
    .bind(&request_org_id)
    .bind(&request_page_id)
    .fetch_one(&service.db_pool);

    #[derive(sqlx::FromRow)]
    struct PageNodeFromRow {
        id: Uuid,
        kind: i32,
        content: String,
        ordering: f64,
    }
    let initial_page_nodes_query = sqlx::query_as(
        "SELECT id, kind, content, ordering \
         FROM page_nodes \
         WHERE org_id = $1 AND page_id = $2 \
         ORDER BY ordering ASC
         LIMIT 25",
    )
    .bind(&request_org_id)
    .bind(&request_page_id)
    .fetch_all(&service.db_pool);

    let (page_meta_result, initial_page_nodes_result): (
        Result<PageFromRow, sqlx::Error>,
        Result<Vec<PageNodeFromRow>, sqlx::Error>,
    ) = futures::join!(page_meta_query, initial_page_nodes_query);
    let page_from_row = page_meta_result.map_err(|_| error::ErrorInternalServerError(""))?;
    let page_nodes_from_row =
        initial_page_nodes_result.map_err(|_| error::ErrorInternalServerError(""))?;

    let response = LoadPageResponse {
        page: Some(Page {
            id: request_page_id.to_simple().to_string(),
            org_id: request_org_id.to_simple().to_string(),
            title: page_from_row.title,
            created_by_user_id: page_from_row.created_by_user_id.to_simple().to_string(),
            last_edited_by_user_id: page_from_row.last_edited_by_user_id.to_simple().to_string(),
            project_owner_user_id: match page_from_row.project_owner_user_id {
                Some(user_id) => user_id.to_simple().to_string(),
                None => "".to_string(),
            },
            created_at: utils::date_time_to_micros(page_from_row.created_at),
            updated_at: utils::date_time_to_micros(page_from_row.updated_at),
        }),
        initial_page_nodes: page_nodes_from_row
            .into_iter()
            .map(|pn| PageNode {
                org_id: request_org_id.to_simple().to_string(),
                page_id: request_page_id.to_simple().to_string(),
                id: pn.id.to_simple().to_string(),
                kind: pn.kind,
                content: pn.content,
                ordering: pn.ordering,
                last_edited_by_user_id: "".to_string(),
            })
            .collect(),
    };

    http::create_protobuf_http_response(&response)
}

/*
#[post("/api/pages.updatePageTitle")]
async fn update_page_title(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api/pages.insertPageNode")]
async fn insert_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api/pages.updatePageNode")]
async fn update_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}

#[post("/api/pages.deletePageNode")]
async fn delete_page_node(id: Identity, service: web::Data<BackendService>) -> impl Responder {
    "unimplemented!"
}
*/

#[cfg(test)]
mod tests {
    use super::*;

    use crate::proto::writing::page_node::Kind;
    use crate::testing::utils::{
        clear_db_tables, create_test_db_pool, create_user, default_backend_service,
        default_identity_service, get_session_cookie, take_response_body,
    };
    use crate::utils;

    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;
    use actix_web::{test, App};
    use sqlx::PgPool;

    #[tokio::test]
    async fn test_create_page_success() {
        let pool = create_test_db_pool().await;
        clear_db_tables(&pool).await;

        let org_id = Uuid::new_v4();
        let user_id = create_user(&pool, &org_id, "janesmith@foo.com", "Jane Smith").await;
        let session_cookie = get_session_cookie(&user_id).await;

        let mut app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_identity_service())
                .service(create_page),
        )
        .await;

        let proto_request = CreatePageRequest {
            title: "Some Awesome Page Title".to_string(),
        };
        let encoded_proto_request = utils::encode_protobuf_message(&proto_request).unwrap();
        let request = TestRequest::post()
            .uri("/api/pages.createPage")
            .header("content-type", "application/protobuf")
            .cookie(session_cookie)
            .set_payload(encoded_proto_request)
            .to_request();

        let mut response = test::call_service(&mut app, request).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/protobuf"
        );
        let response_body = take_response_body(&mut response);
        let proto_response = CreatePageResponse::decode(response_body).unwrap();

        assert!(proto_response.page.is_some());
        let page = proto_response.page.unwrap();
        assert_eq!(page.org_id, org_id.to_simple().to_string());
        assert_eq!(page.created_by_user_id, user_id.to_simple().to_string());
        assert_eq!(page.last_edited_by_user_id, user_id.to_simple().to_string());
        assert_eq!(page.project_owner_user_id, "".to_string());
        assert!(Uuid::parse_str(&page.id).is_ok());
        assert_eq!(page.title, proto_request.title);

        clear_db_tables(&pool).await;
    }

    #[tokio::test]
    async fn test_create_page_not_logged_in() {
        let pool = create_test_db_pool().await;
        clear_db_tables(&pool).await;

        let mut app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_identity_service())
                .service(create_page),
        )
        .await;

        let proto_request = CreatePageRequest {
            title: "Some Awesome Page Title".to_string(),
        };
        let encoded_proto_request = utils::encode_protobuf_message(&proto_request).unwrap();
        // Note no session cookie in the request.
        let request = TestRequest::post()
            .uri("/api/pages.createPage")
            .header("content-type", "application/protobuf")
            .set_payload(encoded_proto_request)
            .to_request();

        let mut response = test::call_service(&mut app, request).await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response_body = take_response_body(&mut response);
        assert!(response_body.is_empty());
    }

    #[tokio::test]
    async fn test_create_page_invalid_protobuf_request() {
        let pool = create_test_db_pool().await;
        clear_db_tables(&pool).await;

        let org_id = Uuid::new_v4();
        let user_id = create_user(&pool, &org_id, "janesmith@foo.com", "Jane Smith").await;
        let session_cookie = get_session_cookie(&user_id).await;

        let mut app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_identity_service())
                .service(create_page),
        )
        .await;

        // Note invalid payload
        let request = TestRequest::post()
            .uri("/api/pages.createPage")
            .header("content-type", "application/protobuf")
            .cookie(session_cookie)
            .set_payload("invalid payload")
            .to_request();

        let mut response = test::call_service(&mut app, request).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response_body = take_response_body(&mut response);
        assert!(response_body.is_empty());
    }

    #[tokio::test]
    async fn test_load_page_success() {
        let pool = create_test_db_pool().await;
        clear_db_tables(&pool).await;

        let org_id = Uuid::new_v4();
        let user_id = create_user(&pool, &org_id, "janesmith@foo.com", "Jane Smith").await;
        let session_cookie = get_session_cookie(&user_id).await;

        let page_title = "Foo Bar";
        let page_id = create_mock_page(&pool, page_title, &org_id, &user_id).await;

        let page_node_contents = vec![
            (Kind::H1, "My Awesome Product"),
            (Kind::H2, "Problem"),
            (Kind::Default, "X is a terrible, painful problem."),
            (Kind::H2, "Solution"),
            (
                Kind::Default,
                "We are excited to announce Y, a solution to problem X.",
            ),
            (Kind::Default, "Here is how to use Y:"),
            (Kind::Code, "def main():\n\tprint('hello world')"),
            (
                Kind::Default,
                "Shout out to Alice and Bob for building this!",
            ),
        ];
        let page_node_ids: Vec<Uuid> =
            futures::future::join_all(page_node_contents.iter().enumerate().map(
                |(i, (kind, content))| {
                    create_mock_page_node(
                        &pool, &org_id, &page_id, *kind, content, i as f64, &user_id,
                    )
                },
            ))
            .await;
        let expected_page_nodes: Vec<(Uuid, Kind, String)> = page_node_ids
            .iter()
            .zip(page_node_contents.iter())
            .map(|(page_node_id, (kind, content))| (*page_node_id, *kind, content.to_string()))
            .collect();

        let mut app = test::init_service(
            App::new()
                .data(default_backend_service().await)
                .wrap(default_identity_service())
                .service(load_page),
        )
        .await;

        let proto_request = LoadPageRequest {
            org_id: org_id.to_simple().to_string(),
            page_id: page_id.to_simple().to_string(),
        };
        let encoded_proto_request = utils::encode_protobuf_message(&proto_request).unwrap();

        // Note invalid payload
        let request = TestRequest::post()
            .uri("/api/pages.loadPage")
            .header("content-type", "application/protobuf")
            .cookie(session_cookie)
            .set_payload(encoded_proto_request)
            .to_request();

        let mut response = test::call_service(&mut app, request).await;

        assert_eq!(response.status(), StatusCode::OK);
        let response_body = take_response_body(&mut response);
        let proto_response = LoadPageResponse::decode(response_body).unwrap();

        assert!(proto_response.page.is_some());
        let page = proto_response.page.unwrap();
        assert_eq!(page.org_id, org_id.to_simple().to_string());
        assert_eq!(page.id, page_id.to_simple().to_string());
        assert_eq!(page.title, page_title);
        assert_eq!(page.created_by_user_id, user_id.to_simple().to_string());
        assert_eq!(page.last_edited_by_user_id, user_id.to_simple().to_string());
        assert!(page.project_owner_user_id.is_empty());

        assert_eq!(
            proto_response.initial_page_nodes.len(),
            expected_page_nodes.len()
        );
        for (i, (page_node_id, kind, content)) in expected_page_nodes.iter().enumerate() {
            let page_node = &proto_response.initial_page_nodes[i];
            assert_eq!(page_node.org_id, org_id.to_simple().to_string());
            assert_eq!(page_node.page_id, page_id.to_simple().to_string());
            assert_eq!(page_node.id, page_node_id.to_simple().to_string());
            assert_eq!(page_node.kind, *kind as i32);
            assert_eq!(page_node.content, *content);
            assert!((page_node.ordering - (i as f64)).abs() < f64::EPSILON);
            // Note: This field is empty. It will be loaded later asynchronously. We elide it from
            // the response so that we do not need to project it into the Postgres index.
            assert!(page_node.last_edited_by_user_id.is_empty());
        }

        clear_db_tables(&pool).await;
    }

    async fn create_mock_page(
        pool: &PgPool,
        page_title: &str,
        org_id: &Uuid,
        user_id: &Uuid,
    ) -> Uuid {
        let (page_id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO pages \
            (org_id, title, created_by_user_id, last_edited_by_user_id, created_at, updated_at) \
            VALUES ($1, $2, $3, $3, now(), now()) \
            RETURNING id",
        )
        .bind(&org_id)
        .bind(page_title)
        .bind(&user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        page_id
    }

    async fn create_mock_page_node(
        pool: &PgPool,
        org_id: &Uuid,
        page_id: &Uuid,
        kind: Kind,
        content: &str,
        ordering: f64,
        last_edited_by_user_id: &Uuid,
    ) -> Uuid {
        let kind_i32: i32 = kind.into();
        let (page_node_id,) : (Uuid,) = sqlx::query_as(
            "INSERT INTO page_nodes \
            (org_id, page_id, kind, content, ordering, last_edited_by_user_id, created_at, updated_at) \
            VALUES ($1, $2, $3, $4, $5, $6, now(), now()) \
            RETURNING id"
        )
        .bind(org_id)
        .bind(page_id)
        .bind(kind_i32)
        .bind(content)
        .bind(ordering)
        .bind(last_edited_by_user_id)
        .fetch_one(pool)
        .await
        .unwrap();
        page_node_id
    }
}
