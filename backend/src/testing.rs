#[cfg(test)]
pub mod utils {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::{Arc, RwLock};

    use crate::http;
    use crate::BackendService;

    use actix_identity::{CookieIdentityPolicy, Identity, IdentityService};
    use actix_web::dev::{Body, ResponseBody, ServiceResponse};
    use actix_web::http::Cookie;
    use actix_web::test;
    use actix_web::test::TestRequest;
    use actix_web::{web, App, HttpResponse};
    use lazy_static::lazy_static;
    use sqlx::postgres::PgPool;
    use sqlx::prelude::PgQueryAs;
    use sqlx::{Cursor, Row};
    use uuid::Uuid;

    pub async fn create_test_db_pool() -> PgPool {
        let password = std::env::var("WRITING_PG_DEV_PASSWORD").unwrap_or_else(|_| {
            panic!("Could not find env var WRITING_PG_DEV_PASSWORD");
        });
        let test_thread_counter = current_test_thread_counter();
        let postgres_config_str = format!(
            "postgres://app_test{}:{}@{}:{}/app_test{}",
            test_thread_counter, &password, "localhost", 5432, test_thread_counter,
        );
        PgPool::builder().build(&postgres_config_str).await.unwrap()
    }

    pub async fn clear_db_tables(pool: &PgPool) {
        let query = "SELECT table_name FROM information_schema.tables \
                     WHERE table_schema = 'public'";
        let mut cursor = sqlx::query(query).fetch(pool);
        let mut table_names = Vec::new();
        while let Some(row) = cursor.next().await.expect("Expected another row") {
            let table_name: &str = row.get("table_name");
            table_names.push(table_name.to_string());
        }
        for table_name in table_names.iter() {
            let query = format!("DELETE FROM {}", table_name);
            sqlx::query(&query)
                .execute(pool)
                .await
                .expect("Expected to delete table");
        }
    }

    pub async fn default_backend_service() -> Arc<BackendService> {
        Arc::new(BackendService {
            db_pool: create_test_db_pool().await,
        })
    }

    pub fn default_identity_service() -> IdentityService<CookieIdentityPolicy> {
        let test_cookie_secret: [u8; 32] = [0; 32];
        http::create_identity_service(&test_cookie_secret, true)
    }

    pub async fn get_session_cookie(user_id: &Uuid) -> Cookie<'static> {
        // This is silly, but if we want to create the session cookie using the actix_identity
        // code, we have to stand up a full http test server and execute a request.
        //
        // It seems like this would be slow, but thankfully it takes less than 1 millisecond under
        // a debug build, less than 100 microseconds under a release build.
        let user_id = user_id.to_owned();
        let mut app = test::init_service(App::new().wrap(default_identity_service()).service(
            web::resource("/test_login").to(move |id: Identity| {
                id.remember(user_id.to_simple().to_string());
                HttpResponse::Ok()
            }),
        ))
        .await;
        let request = TestRequest::with_uri("/test_login").to_request();
        let response = test::call_service(&mut app, request).await;
        let cookie = response.response().cookies().next().unwrap().into_owned();
        cookie
    }

    pub async fn create_user(pool: &PgPool, org_id: &Uuid, email: &str, name: &str) -> Uuid {
        let query_str = "INSERT INTO users \
             (org_id, email, name, created_at, updated_at) \
             VALUES ($1, $2, $3, now(), now()) \
             RETURNING id";
        let result = sqlx::query_as(query_str)
            .bind(org_id)
            .bind(email)
            .bind(name)
            .fetch_one(pool)
            .await;
        let (user_id,): (Uuid,) = result.unwrap();
        user_id
    }

    #[allow(dead_code)]
    pub fn set_log_level(level: log::LevelFilter) {
        simple_logger::SimpleLogger::new()
            .with_level(level)
            .init()
            .unwrap();
    }

    pub fn take_response_body(response: &mut ServiceResponse) -> web::Bytes {
        match response.take_body() {
            ResponseBody::Body(Body::Bytes(bytes)) => Some(bytes),
            _ => None,
        }
        .unwrap()
    }

    lazy_static! {
        static ref TEST_DB_NUMS: Arc<RwLock<HashMap<u64, u16>>> =
            Arc::new(RwLock::new(HashMap::new()));
        static ref TEST_DB_NUM_COUNTER: AtomicU16 = AtomicU16::new(1);
    }

    fn current_test_thread_counter() -> u16 {
        let thread_id = current_thread_id();
        {
            let test_db_nums = TEST_DB_NUMS.read().unwrap();
            if let Some(test_db_num) = test_db_nums.get(&thread_id) {
                return *test_db_num;
            }
        }
        let new_test_db_num = TEST_DB_NUM_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut test_db_nums = TEST_DB_NUMS.write().unwrap();
        test_db_nums.insert(thread_id, new_test_db_num);
        new_test_db_num
    }

    fn current_thread_id() -> u64 {
        // Under the hood, the ThreadId stores a u64. It can only be accessed using the nightly
        // build of Rust. Hence, we use a hack to transform the ThreadId into a debug string and
        // pull the u64 out of the string.
        let thread_id = std::thread::current().id();
        let thread_id = format!("{:?}", thread_id);
        let prefix = "ThreadId(";
        if !thread_id.starts_with(prefix) || !thread_id.ends_with(')') {
            std::panic!("Unexpected ThreadId format: {}", thread_id);
        }
        let thread_id = &thread_id[prefix.len()..thread_id.len() - 1];
        thread_id
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("Unexpected ThreadId format: {}", thread_id))
    }
}
