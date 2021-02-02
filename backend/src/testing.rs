#[cfg(test)]
pub mod utils {
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex};

    use crate::http;
    use crate::BackendService;

    use actix_session::CookieSession;
    use actix_web::dev::{Body, ResponseBody, ServiceResponse};
    use actix_web::web;
    use futures::future;
    use lazy_static::lazy_static;
    use sqlx::postgres::PgPool;
    use sqlx::prelude::PgQueryAs;
    use sqlx::{Cursor, Row};
    use uuid::Uuid;

    /// To make it so test threads do not interfere with one another, each test thread gets its own
    /// Postgres database to work with. At the beginning of a test, you should allocate a new Test
    /// DB pool via the following call:
    ///
    /// ```
    /// let pool = TestDbPool::new().await;
    /// ```
    ///
    /// The current test thread will wait for a Postgres database to become available. When one is
    /// available, all of the tables in the DB will be cleared. The DB will be in a fresh state.
    ///
    /// The `TestDbPool` struct contains a `PgPool` to be used for executing queries (i.e.
    /// `db_pool`) and an `i32` that identifies the DB (i.e. `db_id`).
    ///
    /// Use `default_backend_service(pool.db_id)` to create a backend service whose Postgres
    /// queries talk to the specified DB.
    ///
    /// Use `create_test_db_pool(pool.db_id)` to create a new `PgPool` that talks to the specified
    /// DB.
    ///
    /// When the `TestDbPool` struct is dropped, the DB tables will be cleared again, and the DB
    /// will be made available for other test threads to use.
    const NUM_TEST_DBS: i32 = 8;

    lazy_static! {
        static ref TEST_DB_IDS: Arc<(Mutex<VecDeque<i32>>, Condvar)> = {
            let mut db_ids = VecDeque::new();
            for i in 1..=NUM_TEST_DBS {
                db_ids.push_back(i);
            }
            let mutex = Mutex::new(db_ids);
            let cond_var = Condvar::new();
            Arc::new((mutex, cond_var))
        };
        static ref TEST_DB_PASSWORD: String = std::env::var("WRITING_PG_DEV_PASSWORD")
            .unwrap_or_else(|_| {
                panic!("Could not find env var WRITING_PG_DEV_PASSWORD");
            });
    }

    pub struct TestDbPool {
        pub db_pool: PgPool,
        pub db_id: i32,
    }

    impl TestDbPool {
        pub async fn new() -> Self {
            let db_id = {
                let db_ids_mutex = &TEST_DB_IDS.0;
                let cond_var = &TEST_DB_IDS.1;

                let mut db_ids = db_ids_mutex.lock().unwrap();
                while db_ids.is_empty() {
                    db_ids = cond_var.wait(db_ids).unwrap();
                }
                db_ids.pop_front().unwrap()
            };

            let db_pool = create_test_db_pool(db_id).await;
            clear_test_db_tables(&db_pool).await;

            TestDbPool { db_pool, db_id }
        }
    }

    impl Drop for TestDbPool {
        fn drop(&mut self) {
            futures::executor::block_on(clear_test_db_tables(&self.db_pool));

            let db_ids_mutex = &TEST_DB_IDS.0;
            let cond_var = &TEST_DB_IDS.1;

            {
                let mut db_ids = db_ids_mutex.lock().unwrap();
                db_ids.push_back(self.db_id);
            }
            cond_var.notify_one();
        }
    }

    pub async fn create_test_db_pool(db_id: i32) -> PgPool {
        let postgres_config_str = format!(
            "postgres://app_test{}:{}@{}:{}/app_test{}",
            db_id, &*TEST_DB_PASSWORD, "localhost", 5432, db_id,
        );
        PgPool::builder().build(&postgres_config_str).await.unwrap()
    }

    pub async fn clear_test_db_tables(pool: &PgPool) {
        let query = "SELECT table_name FROM information_schema.tables \
                     WHERE table_schema = 'public'";
        let mut cursor = sqlx::query(query).fetch(pool);
        let mut table_names = Vec::new();
        while let Some(row) = cursor.next().await.expect("Expected another row") {
            let table_name: &str = row.get("table_name");
            table_names.push(table_name.to_string());
        }
        let mut futures = Vec::new();
        for table_name in table_names.iter().cloned() {
            let query = format!("DELETE FROM {}", table_name);
            futures.push(async move { sqlx::query(&query).execute(pool).await });
        }
        future::join_all(futures).await;
    }

    pub async fn default_backend_service(db_id: i32) -> Arc<BackendService> {
        Arc::new(BackendService {
            db_pool: create_test_db_pool(db_id).await,
        })
    }

    pub const TEST_COOKIE_SECRET: [u8; 32] = [0; 32];

    pub fn default_cookie_session() -> CookieSession {
        http::create_cookie_session(&TEST_COOKIE_SECRET, false)
    }

    pub fn set_session_cookie(session: &actix_session::Session, org_id: &Uuid, user_id: &Uuid) {
        session
            .set("org_id", org_id.to_simple().to_string())
            .unwrap();
        session
            .set("user_id", user_id.to_simple().to_string())
            .unwrap();
    }

    pub fn decrypt_session_cookie_value(
        session_cookie: &cookie::Cookie,
        name: &str,
    ) -> Option<String> {
        let session_cookie = session_cookie.clone().into_owned();
        let key = cookie::Key::derive_from(&TEST_COOKIE_SECRET);
        let mut cookie_jar = cookie::CookieJar::new();
        cookie_jar.add_original(session_cookie);
        match cookie_jar.private(&key).get(name) {
            Some(cookie) => Some(cookie.value().to_string()),
            None => None,
        }
    }

    pub async fn create_user(pool: &PgPool, org_id: &Uuid, email: &str, name: &str) -> Uuid {
        let (user_id,): (Uuid,) = sqlx::query_as(
            "INSERT INTO users \
             (email, name, created_at, updated_at) \
             VALUES ($1, $2, now(), now()) \
             RETURNING id",
        )
        .bind(email)
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO organization_users \
            (org_id, user_id, created_at, updated_at) \
            VALUES ($1, $2, now(), now())",
        )
        .bind(org_id)
        .bind(&user_id)
        .execute(pool)
        .await
        .unwrap();

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
}
