#[cfg(test)]

pub mod dynamodb;

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

    /// To prevent test threads from interfering with one another, each thread should get its own
    /// Postgres database to work with.
    ///
    /// You can use `TestDbPool` in your tests to achieve this.
    ///
    /// Usage:
    ///
    /// ```
    /// #[tokio::test]
    /// async fn test_my_feature() {
    ///     // Allocate a new DB pool for this test. The thread waits until a DB is available. When
    ///     // available, all tables in the DB are cleared, and a connection pool to the DB is
    ///     // returned.
    ///     let pool = TestDbPool::new().await;
    ///
    ///     // Use `pool.db_pool()` to get a `&PgPool` for sqlx queries.
    ///     sqlx::query("UPDATE foo SET bar = 'baz' WHERE id = 123")
    ///         .execute(db.db_pool())
    ///         .await;
    ///
    ///     // Use `pool.db_pool_clone()` to get an `Arc<PgPool>` reference to the pool.
    ///     //
    ///     // For example, you can give this to a `BackendService` instance so that your backend
    ///     // code can use the same DB connection pool as your test code.
    ///     let app = test::init_service(
    ///         App::new()
    ///             .data(default_backend_service(pool.db_pool_clone()))
    ///             .wrap(default_cookie_session())
    ///             .service(route_that_handles_my_feature)
    ///     ).await;
    ///
    ///     // When the `TestDbPool` is dropped, all tables in the DB are cleared once more.
    /// }
    /// ```
    const NUM_TEST_DBS: i32 = 8;

    // Pair of (lazily initialized Postgres connection pool, DB id).
    type TestDbPair = (Option<Arc<PgPool>>, i32);

    lazy_static! {
        static ref TEST_DB_POOLS: Arc<(Mutex<VecDeque<TestDbPair>>, Condvar)> = {
            let mut pools = VecDeque::new();
            for i in 1..=NUM_TEST_DBS {
                pools.push_back((None, i));
            }
            let pools_mutex = Mutex::new(pools);
            let cond_var = Condvar::new();
            Arc::new((pools_mutex, cond_var))
        };
        static ref TEST_DB_PASSWORD: String = std::env::var("WRITING_PG_DEV_PASSWORD")
            .unwrap_or_else(|_| {
                panic!("Could not find env var WRITING_PG_DEV_PASSWORD");
            });
    }

    pub struct TestDbPool {
        db_pool: Arc<PgPool>,
        db_id: i32,
    }

    impl TestDbPool {
        pub async fn new() -> Self {
            let (pool_opt, db_id) = {
                let pools_mutex = &TEST_DB_POOLS.0;
                let cond_var = &TEST_DB_POOLS.1;

                let mut pools = pools_mutex.lock().unwrap();
                while pools.is_empty() {
                    pools = cond_var.wait(pools).unwrap();
                }
                pools.pop_front().unwrap()
            };

            let db_pool = match pool_opt {
                Some(db_pool) => db_pool,
                None => Arc::new(create_test_db_pool(db_id).await),
            };
            let ret = TestDbPool { db_pool, db_id };
            clear_test_db_tables(ret.db_pool()).await;
            ret
        }

        pub fn db_pool(&self) -> &PgPool {
            &*self.db_pool
        }

        pub fn db_pool_clone(&self) -> Arc<PgPool> {
            self.db_pool.clone()
        }
    }

    impl Drop for TestDbPool {
        fn drop(&mut self) {
            futures::executor::block_on(clear_test_db_tables(self.db_pool()));
            let pools_mutex = &TEST_DB_POOLS.0;
            let cond_var = &TEST_DB_POOLS.1;
            {
                let mut pools = pools_mutex.lock().unwrap();
                pools.push_back((Some(self.db_pool.clone()), self.db_id));
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

    pub async fn default_backend_service(db_pool: Arc<PgPool>) -> BackendService {
        BackendService {
            db_pool,
            dynamodb_client: rusoto_dynamodb::DynamoDbClient::new(
                rusoto_core::Region::Custom {
                    name: "testing".to_string(),
                    endpoint: "http://localhost:8000".to_string(),
                }),
        }
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
