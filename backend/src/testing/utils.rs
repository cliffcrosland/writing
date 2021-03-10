use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use actix_session::CookieSession;
use actix_web::dev::{Body, ResponseBody, ServiceResponse};
use actix_web::web;
use lazy_static::lazy_static;
use rusoto_dynamodb::{DeleteTableInput, DynamoDb, DynamoDbClient};
use uuid::Uuid;

use crate::dynamodb::test_table_name;
use crate::http;
use crate::BackendService;

const NUM_TEST_DYNAMODB_SHARDS: i32 = 8;

lazy_static! {
    static ref TEST_DYNAMODB_SHARDS: Arc<(Mutex<VecDeque<i32>>, Condvar)> = {
        let mut shards = VecDeque::new();
        for shard in 1..=NUM_TEST_DYNAMODB_SHARDS {
            shards.push_back(shard);
        }
        let shards_mutex = Mutex::new(shards);
        let cond_var = Condvar::new();
        Arc::new((shards_mutex, cond_var))
    };
}

thread_local! {
    static CURRENT_TEST_THREAD_DYNAMODB_SHARD: RefCell<i32> = RefCell::new(-1);
}

pub fn current_test_thread_dynamodb_shard() -> i32 {
    let value = CURRENT_TEST_THREAD_DYNAMODB_SHARD.with(|s| *s.borrow());
    assert!(value >= 0);
    value
}

fn set_current_test_thread_dynamodb_shard(shard: i32) {
    CURRENT_TEST_THREAD_DYNAMODB_SHARD.with(|s| {
        s.replace(shard);
    });
}

fn clear_current_test_thread_dynamodb_shard() {
    CURRENT_TEST_THREAD_DYNAMODB_SHARD.with(|s| {
        s.replace(-1);
    });
}

pub struct TestDynamoDb {
    pub dynamodb_shard: i32,
    pub dynamodb_client: DynamoDbClient,
}

impl TestDynamoDb {
    pub async fn new() -> Self {
        let dynamodb_shard = {
            let shards_mutex = &TEST_DYNAMODB_SHARDS.0;
            let cond_var = &TEST_DYNAMODB_SHARDS.1;
            let mut shards = shards_mutex.lock().unwrap();
            while shards.is_empty() {
                shards = cond_var.wait(shards).unwrap();
            }
            shards.pop_front().unwrap()
        };

        let dynamodb_client = create_test_dynamodb_client();

        set_current_test_thread_dynamodb_shard(dynamodb_shard);
        delete_test_tables(dynamodb_shard, &dynamodb_client).await;
        create_test_tables(dynamodb_shard, &dynamodb_client).await;

        TestDynamoDb {
            dynamodb_shard,
            dynamodb_client,
        }
    }
}

fn create_test_dynamodb_client() -> DynamoDbClient {
    // NOTE: Must create a new HTTP client per test tokio executor to avoid panics in hyper http
    // code. See details: https://github.com/hyperium/hyper/issues/2112
    let request_dispatcher = rusoto_core::request::HttpClient::new().unwrap();
    let credentials_provider = rusoto_credential::DefaultCredentialsProvider::new().unwrap();
    let region = rusoto_core::Region::Custom {
        name: "testing".to_string(),
        endpoint: "http://localhost:8000".to_string(),
    };
    DynamoDbClient::new_with(request_dispatcher, credentials_provider, region)
}

impl Drop for TestDynamoDb {
    fn drop(&mut self) {
        let shards_mutex = &TEST_DYNAMODB_SHARDS.0;
        let cond_var = &TEST_DYNAMODB_SHARDS.1;
        {
            let mut shards = shards_mutex.lock().unwrap();
            shards.push_back(self.dynamodb_shard);
        }
        cond_var.notify_one();
        clear_current_test_thread_dynamodb_shard();
    }
}

async fn create_test_tables(dynamodb_shard: i32, dynamodb_client: &dyn DynamoDb) {
    for table_def in crate::dynamodb::schema::TABLE_DEFINITIONS.iter() {
        // Local DynamoDB sometimes experiences ephemeral errors when creating tables. Retry a few
        // times until we succeed. Sleep briefly between attempts.
        let mut success = false;
        for _ in 1..=5 {
            let mut table_def = table_def.clone();
            table_def.table_name = test_table_name(dynamodb_shard, &table_def.table_name);
            let result = dynamodb_client.create_table(table_def).await;
            if result.is_ok() {
                success = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(success);
    }
}

async fn delete_test_tables(dynamodb_shard: i32, dynamodb_client: &dyn DynamoDb) {
    for table_def in crate::dynamodb::schema::TABLE_DEFINITIONS.iter() {
        let table_name = test_table_name(dynamodb_shard, &table_def.table_name);
        let _result = dynamodb_client
            .delete_table(DeleteTableInput {
                table_name: table_name.clone(),
            })
            .await;
    }
}

pub async fn default_backend_service() -> BackendService {
    BackendService {
        dynamodb_client: Arc::new(create_test_dynamodb_client()),
    }
}

pub const TEST_COOKIE_SECRET: [u8; 32] = [0; 32];

pub fn default_cookie_session() -> CookieSession {
    http::create_cookie_session(&TEST_COOKIE_SECRET, false)
}

#[allow(dead_code)]
pub fn set_session_cookie(session: &actix_session::Session, org_id: &Uuid, user_id: &Uuid) {
    session
        .set("org_id", org_id.to_hyphenated().to_string())
        .unwrap();
    session
        .set("user_id", user_id.to_hyphenated().to_string())
        .unwrap();
}

pub fn decrypt_session_cookie_value(session_cookie: &cookie::Cookie, name: &str) -> Option<String> {
    let session_cookie = session_cookie.clone().into_owned();
    let key = cookie::Key::derive_from(&TEST_COOKIE_SECRET);
    let mut cookie_jar = cookie::CookieJar::new();
    cookie_jar.add_original(session_cookie);
    match cookie_jar.private(&key).get(name) {
        Some(cookie) => Some(cookie.value().to_string()),
        None => None,
    }
}

#[allow(dead_code)]
pub fn set_log_level(level: log::LevelFilter) {
    simple_logger::SimpleLogger::new()
        .with_level(level)
        .init()
        .unwrap();
}

#[allow(dead_code)]
pub fn take_response_body(response: &mut ServiceResponse) -> web::Bytes {
    match response.take_body() {
        ResponseBody::Body(Body::Bytes(bytes)) => Some(bytes),
        _ => None,
    }
    .unwrap()
}
