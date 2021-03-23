// TODO(cliff): Do we still need this?
#![allow(dead_code)]
//! Distributed locking, powered by DynamoDB.
//!
//! # Problem
//!
//! Let's say that multiple clients want to access a resource, but only one client should access it
//! at a time.
//!
//! # Solution
//!
//! The clients can coordinate using a lock.
//!
//! A client can acquire a lock. No other client is allowed to acquire the lock until it has been
//! released.
//!
//! A lock can be released by the client that holds it.
//!
//! Also, the lock can automatically release itself. When a client acquires a lock, it receives a
//! temporary lease on the lock. When the lease expires, the lock is released. This helps us handle
//! cases where clients crash before releasing their locks.
//!
//! When a client acquires a lock, it can ask for the lease to be renewed periodically in the
//! background. That way, the client will hold the lock until either the client releases it or the
//! client crashes.
//!
//! # Implementation
//!
//! We use DynamoDB update conditional expressions to make lock acquisition atomic. If the
//! conditional expression check fails, we know that someone beat us to the lock.
//!
//! To prevent clock skew errors, we use lease durations instead of lease start/end timestamps.
//! When a client tries to acquire a lock and discovers that someone else holds it, the client
//! makes note of the lease duration. The client then waits for that duration or longer before
//! trying again. The lease is guaranteed to have expired in the meantime.

use std::error::Error;
use std::fmt;
use std::string::ToString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::dynamodb::{av_get_n, av_get_s, av_map, av_n, av_s, table_name};
use crate::ids::{Id, IdType};

use rusoto_core::RusotoError;
use rusoto_dynamodb::{
    DeleteItemError, DeleteItemInput, DynamoDb, DynamoDbClient, GetItemError, GetItemInput,
    UpdateItemError, UpdateItemInput,
};
use tokio::sync::Notify;

/// Client to acquire and release locks.
pub struct LockClient {
    dynamodb_client: Arc<DynamoDbClient>,
}

/// Returned when a lock is successfully acquired. Pass this to `release_lock`.
pub struct Lock {
    pub lock_key: String,
    lease: Lease,
    lease_renewer: Option<Arc<LeaseRenewer>>,
}

#[derive(Clone)]
enum Lease {
    Fixed(LeaseDetails),
    Renewable(Arc<Mutex<LeaseDetails>>),
}

/// Details about a lease, including who holds it, how long the lease duration is, and an instant
/// that is after the lease began.
#[derive(Clone)]
pub struct LeaseDetails {
    pub lock_key: String,
    pub client_id: String,
    pub lease_id: String,
    pub lease_duration: Duration,
    pub lease_started_before: Instant,
}

/// The result returned when a client tries to acquire a lock.
pub enum AcquireLockResult {
    /// You successfully acquired the lock.
    Acquired(Lock),

    /// Someone else holds the lock.
    Conflict(Conflict),
}

/// The kind of lock conflict.
pub enum Conflict {
    /// We discovered that the lock was held by someone else's client. Here are details about that
    /// client's lease.
    KnownHolder(LeaseDetails),

    /// Our atomic lock acquisition attempt was rejected. All we know is that someone else jumped
    /// in before us to acquire the lock, but we do not know anything about their lease.
    UnknownHolder,
}

/// Options to pass in when trying to acquire a lock.
#[derive(Default)]
pub struct LockOpts {
    /// Unique identifier of a lock.
    pub lock_key: String,

    /// Unique identifier of a client. Tells us who is holding the lock. A good value here might
    /// be something like "<host>:<feature>:<random_uuid>".
    ///
    /// Logically distinct clients must have distinct client ids. Clients with the same id are
    /// considered to be the same client and can steal the lock from one another, which breaks
    /// mutual exclusion.
    ///
    /// NOTE: A thread id is *not* a good client id. This code is async, and the thread that starts
    /// an asynchronous call may not be the thread that finishes it. Using thread id as your client
    /// id will almost certainly break mutual exclusion.
    pub client_id: String,

    /// Duration of the client's lease on the lock before it expires. If a client acquires a lock
    /// and then crashes before releasing it, the lock will become available after the lease
    /// expires.
    pub lease_duration: Duration,

    /// If `renew_lease_interval` is given, the client will automatically renew the lease in the
    /// background periodically on this interval.
    ///
    /// This duration must be shorter than the `lease_duration` to be effective. Otherwise, the
    /// lease will expire before it can be renewed.
    pub renew_lease_interval: Option<Duration>,

    /// If we receive an `AcquireLockResult::Conflict(Conflict::KnownHolder(lease_details))` result
    /// in a prior call to `try_acquire_lock`, and we want to try to acquire the same lock again,
    /// we should pass in the `lease_details` from that result here.
    ///
    /// Case 1: If our client owns the lease on the lock, then we are allowed to renew the lease
    /// before it expires.
    ///
    /// Case 2: If someone else's client owns the lease on the lock, then we can only acquire the
    /// lock after their lease has expired.
    pub prev_lease_details: Option<LeaseDetails>,
}

/// Worker that automatically renews a lease in the background. As long as the client does not
/// crash, the client's ownership of the lock can be renewed indefinitely.
struct LeaseRenewer {
    lock_client: LockClient,
    renewable_lease: Arc<Mutex<LeaseDetails>>,
    renew_lease_interval: Duration,
    running: AtomicBool,
    awake_from_sleep: Arc<Notify>,
}

impl LockClient {
    pub fn new(dynamodb_client: Arc<DynamoDbClient>) -> Self {
        Self { dynamodb_client }
    }

    /// Try to acquire a lock.
    ///
    /// ### Lock Success
    ///
    /// If the lock was acquired, returns `Ok(AcquireLockResult::Acquired(lock))`. Pass this `lock`
    /// value to `release_lock` later.
    ///
    /// When `opts.renew_lease_interval` is present, the client will automatically renew the lease
    /// in the background on that interval.
    ///
    /// ### Lock Conflict
    ///
    /// If someone else holds the lock, returns `Ok(AcquireLockResult::Conflict(conflict))`.
    ///
    /// The `conflict` value can be `Conflict::KnownHolder(lease_details)` or
    /// `Conflict::UnknownHolder`.
    ///
    /// `Conflict::KnownHolder(lease_details)` happens when we peek at the lock and notice that
    /// someone holds it. Pass in `lease_details` in a future call to `try_acquire_lock` via the
    /// `opts.prev_lease_details` field. This will help you acquire the lock if the lease has
    /// expired.
    ///
    /// `Conflict::UnknownHolder` happens when our atomic lock acquisition fails because someone
    /// beat us to the lock.
    ///
    ///
    /// ### Error
    ///
    /// If a DynamoDB error occurrs, returns `Err(LockError)`.
    pub async fn try_acquire_lock(&self, opts: &LockOpts) -> Result<AcquireLockResult, LockError> {
        let db_table_name = table_name("advisory_locks");
        let db_key = av_map(&[av_s("lock_key", &opts.lock_key)]);

        // Take a peek at the lock before attempting to acquire it.
        //
        // If the lock is held by someone else, we make note of the lease duration so we know how
        // long to wait before trying again.
        if opts.prev_lease_details.is_none() {
            let input = GetItemInput {
                table_name: db_table_name.clone(),
                key: db_key.clone(),
                consistent_read: Some(true),
                projection_expression: Some(String::from("lease_id, client_id, lease_duration_ms")),
                ..Default::default()
            };
            let result = self.dynamodb_client.get_item(input).await;
            match result {
                Ok(output) => {
                    if let Some(item) = output.item {
                        let lease_id = av_get_s(&item, "lease_id").unwrap_or("");
                        let client_id = av_get_s(&item, "client_id").unwrap_or("");
                        let lease_duration_ms =
                            av_get_n::<u64>(&item, "lease_duration_ms").unwrap_or(0);
                        return Ok(AcquireLockResult::Conflict(Conflict::KnownHolder(
                            LeaseDetails {
                                lock_key: opts.lock_key.clone(),
                                client_id: String::from(client_id),
                                lease_id: String::from(lease_id),
                                lease_duration: Duration::from_millis(lease_duration_ms),
                                lease_started_before: Instant::now(),
                            },
                        )));
                    }
                }
                Err(e) => {
                    return Err(LockError::GetItemError(e));
                }
            }
        }

        // If the lock is held by another client, we must wait until after that client's lease has
        // expired to try to acquire it.
        if let Some(prev_lease_details) = opts.prev_lease_details.as_ref() {
            if prev_lease_details.client_id != opts.client_id && !prev_lease_details.is_expired() {
                return Ok(AcquireLockResult::Conflict(Conflict::KnownHolder(
                    prev_lease_details.clone(),
                )));
            }
        }

        // If we get here, we think:
        // - no one owns the lock, or
        // - the prior owner's lease expired, or
        // - we own the lock and are renewing our lease.
        //
        // Attempt to acquire the lock atomically. Use condition expression to fail if someone
        // else acquired the lock before us.
        let new_lease_id = Id::new(IdType::LockLease).as_str().to_string();
        let input = UpdateItemInput {
            table_name: db_table_name,
            key: db_key,
            update_expression: Some(String::from(
                "SET lease_id = :lease_id, client_id = :client_id, \
                lease_duration_ms = :lease_duration_ms",
            )),
            condition_expression: Some(String::from(
                "attribute_not_exists(lease_id) OR lease_id = :prev_lease_id",
            )),
            expression_attribute_values: Some(av_map(&[
                av_s(":lease_id", new_lease_id.as_str()),
                av_s(
                    ":prev_lease_id",
                    opts.prev_lease_details
                        .as_ref()
                        .map(|ld| ld.lease_id.as_str())
                        .unwrap_or(""),
                ),
                av_s(":client_id", &opts.client_id),
                av_n(":lease_duration_ms", opts.lease_duration.as_millis()),
            ])),
            ..Default::default()
        };
        let result = self.dynamodb_client.update_item(input).await;
        match result {
            Ok(_) => {
                let lease_details = LeaseDetails {
                    lock_key: opts.lock_key.clone(),
                    client_id: opts.client_id.clone(),
                    lease_id: new_lease_id,
                    lease_duration: opts.lease_duration,
                    lease_started_before: Instant::now(),
                };
                let (lease, lease_renewer) = match opts.renew_lease_interval {
                    None => (Lease::Fixed(lease_details), None),
                    Some(interval) => {
                        let renewable_lease = Arc::new(Mutex::new(lease_details));
                        let lease_renewer = LeaseRenewer::new(
                            self.dynamodb_client.clone(),
                            renewable_lease.clone(),
                            interval,
                        );
                        (Lease::Renewable(renewable_lease), Some(lease_renewer))
                    }
                };

                Ok(AcquireLockResult::Acquired(Lock {
                    lock_key: opts.lock_key.clone(),
                    lease,
                    lease_renewer,
                }))
            }
            Err(RusotoError::Service(UpdateItemError::ConditionalCheckFailed(_))) => {
                Ok(AcquireLockResult::Conflict(Conflict::UnknownHolder))
            }
            Err(e) => Err(LockError::UpdateItemError(e)),
        }
    }

    /// Release a lock. Pass in a reference to the `lock` value returned by a previous successful
    /// call to `try_acquire_lock`.
    ///
    /// ### Success
    ///
    /// If our lease is still valid, the lease is deleted from DynamoDB. We use conditional updates
    /// to make sure that we do not delete someone else's lease.
    ///
    /// If our lease expired and someone else now holds a lease on the lock, that is also fine.
    ///
    /// ### Error
    ///
    /// Return `Err(LockError)` if we experience failure communicating with DynamoDB.
    pub async fn release_lock(&self, lock: &Lock) -> Result<(), LockError> {
        let lease_details = lock.lease_details();
        let input = DeleteItemInput {
            table_name: table_name("advisory_locks"),
            key: av_map(&[av_s("lock_key", &lease_details.lock_key)]),
            condition_expression: Some(String::from("lease_id = :lease_id")),
            expression_attribute_values: Some(av_map(&[av_s(
                ":lease_id",
                &lease_details.lease_id,
            )])),
            ..Default::default()
        };
        let result = self.dynamodb_client.delete_item(input).await;
        match result {
            Ok(_) | Err(RusotoError::Service(DeleteItemError::ConditionalCheckFailed(_))) => {
                // Either we held the lock and successfully released it, or someone else acquired
                // it because our lease expired. In either case, we no longer own it.
                if let Some(lease_renewer) = lock.lease_renewer.as_ref() {
                    lease_renewer.stop();
                }
                Ok(())
            }
            Err(e) => Err(LockError::DeleteItemError(e)),
        }
    }
}

impl LeaseRenewer {
    fn new(
        dynamodb_client: Arc<DynamoDbClient>,
        renewable_lease: Arc<Mutex<LeaseDetails>>,
        renew_lease_interval: Duration,
    ) -> Arc<Self> {
        let lease_renewer = Arc::new(Self {
            lock_client: LockClient::new(dynamodb_client),
            renewable_lease,
            renew_lease_interval,
            running: AtomicBool::new(true),
            awake_from_sleep: Arc::new(Notify::new()),
        });
        Self::start(lease_renewer.clone());
        lease_renewer
    }

    fn start(lease_renewer: Arc<LeaseRenewer>) {
        tokio::spawn(async move {
            // TODO(cliff): Renewer runs forever until it is explicitly stopped. Is that bad? What
            // if some thread crashes before sending the signal to stop the renewer?
            while lease_renewer.is_running() {
                let sleep_for = lease_renewer.renew_lease_interval;
                let awake_from_sleep = lease_renewer.awake_from_sleep.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(sleep_for).await;
                    awake_from_sleep.notify_one();
                });
                // Notified when sleep ends *or* when renewer has been stopped.
                lease_renewer.awake_from_sleep.notified().await;
                if !lease_renewer.is_running() {
                    break;
                }
                lease_renewer.renew_lease().await;
            }
        });
    }

    async fn renew_lease(&self) {
        let lock_opts = {
            let lease_details = self.renewable_lease.lock().unwrap();
            LockOpts {
                lock_key: lease_details.lock_key.clone(),
                client_id: lease_details.client_id.clone(),
                prev_lease_details: Some(lease_details.clone()),
                lease_duration: lease_details.lease_duration,
                // We do not want to recursively renew the lease.
                renew_lease_interval: None,
            }
        };
        let result = self.lock_client.try_acquire_lock(&lock_opts).await;
        match result {
            Ok(AcquireLockResult::Acquired(lock)) => match lock.lease {
                Lease::Fixed(lease_details) => {
                    // Lease renewal successful.
                    let mut renewable_lease_details = self.renewable_lease.lock().unwrap();
                    *renewable_lease_details = lease_details;
                }
                Lease::Renewable(_) => {
                    debug_assert!(false, "Recursive renewable lease! Should never happen.");
                    log::info!("Invalid renewable lease. Stopping lease renewer.");
                    self.stop();
                }
            },
            Ok(AcquireLockResult::Conflict(_)) => {
                log::info!(
                    "Can no longer renew lock {}. Held by another client. Stopping lease renewer.",
                    &lock_opts.lock_key
                );
                self.stop();
            }
            Err(e) => {
                log::error!(
                    "Error occurred attempting to renew lease on {}. Stopping lease renewer. Error: {}",
                    &lock_opts.lock_key,
                    &e
                );
                self.stop();
            }
        }
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.awake_from_sleep.notify_one();
    }
}

impl Lock {
    fn lease_details(&self) -> LeaseDetails {
        match &self.lease {
            Lease::Fixed(lease_details) => lease_details.clone(),
            Lease::Renewable(lease_details) => {
                let lease_details = lease_details.lock().unwrap();
                lease_details.clone()
            }
        }
    }
}

impl LeaseDetails {
    fn is_expired(&self) -> bool {
        let elapsed = Instant::now().duration_since(self.lease_started_before);
        elapsed > self.lease_duration
    }
}

#[derive(Debug)]
pub enum LockError {
    DeleteItemError(RusotoError<DeleteItemError>),
    GetItemError(RusotoError<GetItemError>),
    UpdateItemError(RusotoError<UpdateItemError>),
}

impl fmt::Display for LockError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let error_string = match *self {
            LockError::DeleteItemError(ref err) => err.to_string(),
            LockError::GetItemError(ref err) => err.to_string(),
            LockError::UpdateItemError(ref err) => err.to_string(),
        };
        write!(f, "LockError {}", &error_string)
    }
}

impl Error for LockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            LockError::DeleteItemError(ref err) => Some(err),
            LockError::GetItemError(ref err) => Some(err),
            LockError::UpdateItemError(ref err) => Some(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::utils::TestDynamoDb;

    #[tokio::test]
    async fn test_simple_acquire_and_release_lock() {
        let db = TestDynamoDb::new().await;

        let lock_key = "testkey".to_string();
        let client_id = "client123".to_string();
        let lease_duration = Duration::from_millis(1000);

        let lock_client = LockClient::new(Arc::new(db.dynamodb_client.clone()));

        // 1. Acquire lock
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: client_id.clone(),
                lease_duration,
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        let result = result.unwrap();
        let lock = match result {
            AcquireLockResult::Acquired(lock) => {
                assert_eq!(&lock.lock_key, &lock_key);
                let lease_details = lock.lease_details();
                assert_eq!(&lease_details.lock_key, &lock_key);
                assert_eq!(&lease_details.client_id, &client_id);
                assert!(!lease_details.lease_id.is_empty());
                assert_eq!(lease_details.lease_duration, lease_duration);
                let elapsed = Instant::now().duration_since(lease_details.lease_started_before);
                assert!(elapsed.as_nanos() > 0);
                lock
            }
            _ => {
                unreachable!("Lock was not acquired!")
            }
        };

        // 2. Verify that lock entry appears in DB.
        let result = db
            .dynamodb_client
            .get_item(GetItemInput {
                table_name: table_name("advisory_locks"),
                key: av_map(&[av_s("lock_key", &lock.lock_key)]),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().item.is_some());

        // 3. Release lock.
        let result = lock_client.release_lock(&lock).await;
        assert!(result.is_ok());

        // 4. Verify that lock entry no longer appears in DB.
        let result = db
            .dynamodb_client
            .get_item(GetItemInput {
                table_name: table_name("advisory_locks"),
                key: av_map(&[av_s("lock_key", &lock.lock_key)]),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().item.is_none());
    }

    #[tokio::test]
    async fn test_lease_renewal() {
        let db = TestDynamoDb::new().await;

        let time = 50;
        let epsilon = 10;

        let lock_key = "foobar".to_string();
        let client_id = "test-client".to_string();

        let lock_client = LockClient::new(Arc::new(db.dynamodb_client.clone()));

        // 1. Acquire a lock for duration T. Set up to renew lease in the background on an interval
        //    of T - epsilon.
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: client_id.clone(),
                lease_duration: Duration::from_millis(time),
                renew_lease_interval: Some(Duration::from_millis(time - epsilon)),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        let result = result.unwrap();
        let lock = match result {
            AcquireLockResult::Acquired(lock) => lock,
            _ => unreachable!("Lock was not acquired!"),
        };

        // 2. Check that we hold the lock and that the lease renewer is running.
        let lease_details = lock.lease_details();
        assert_eq!(&lock_key, &lease_details.lock_key);
        assert_eq!(&client_id, &lease_details.client_id);
        assert!(!lease_details.is_expired());
        let lease_id1 = lease_details.lease_id.clone();
        let lease_start1 = lease_details.lease_started_before;
        assert!(lock.lease_renewer.is_some());
        assert!(lock.lease_renewer.as_ref().unwrap().is_running());

        // 3. Sleep for T + epsilon. Check that lease was renewed in background.
        tokio::time::sleep(Duration::from_millis(time + epsilon)).await;

        let lease_details = lock.lease_details();
        assert_eq!(&lock_key, &lease_details.lock_key);
        assert_eq!(&client_id, &lease_details.client_id);
        assert!(!lease_details.is_expired());
        let lease_id2 = lease_details.lease_id.clone();
        let lease_start2 = lease_details.lease_started_before;
        assert_ne!(&lease_id1, &lease_id2);
        assert!(lease_start2 > lease_start1);

        // 4. Release lock.
        let result = lock_client.release_lock(&lock).await;
        assert!(result.is_ok());

        // 5. Check that the lease renewer is no longer running.
        assert!(lock.lease_renewer.is_some());
        assert!(!lock.lease_renewer.as_ref().unwrap().is_running());

        // 6. Sleep for time T. Check that the lease expired without being renewed.
        tokio::time::sleep(Duration::from_millis(time)).await;
        let lease_details = lock.lease_details();
        assert!(lease_details.is_expired());
        assert_eq!(&lease_details.lease_id, &lease_id2);
    }

    #[tokio::test]
    async fn test_lock_conflict() {
        let db = TestDynamoDb::new().await;

        let lock_client = LockClient::new(Arc::new(db.dynamodb_client.clone()));

        let lock_key = "foobar".to_string();
        let alice = "alice".to_string();
        let bob = "bob".to_string();
        let time = 50;

        // 1. Alice acquires lock
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: alice.clone(),
                lease_duration: Duration::from_millis(time),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        match result.unwrap() {
            AcquireLockResult::Acquired(lock) => {
                assert_eq!(&lock.lease_details().client_id, &alice);
                assert!(!lock.lease_details().is_expired());
            }
            _ => unreachable!("Lock was not acquired!"),
        }

        // 2. Bob tries to acquire the lock but finds that Alice owns it.
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: bob.clone(),
                lease_duration: Duration::from_millis(time),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        let lease_details = match result.unwrap() {
            AcquireLockResult::Conflict(Conflict::KnownHolder(lease_details)) => {
                assert_eq!(&lease_details.client_id, &alice);
                assert!(!lease_details.lease_id.is_empty());
                assert_eq!(lease_details.lease_duration, Duration::from_millis(time));
                assert!(lease_details.lease_started_before < Instant::now());
                lease_details
            }
            _ => unreachable!("Bob should not have acquired lock while Alice holds it"),
        };

        // 3. Bob sleeps until lease has expired and tries to acquire lock again. Alice never
        //    renewed her lease on the lock, so Bob should succesfully acquire it.
        tokio::time::sleep(lease_details.lease_duration).await;
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: bob.clone(),
                lease_duration: Duration::from_millis(time),
                prev_lease_details: Some(lease_details.clone()),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        match result.unwrap() {
            AcquireLockResult::Acquired(lock) => {
                assert_eq!(&lock.lease_details().client_id, &bob);
                assert!(!lock.lease_details().is_expired());
            }
            _ => unreachable!("Failed to acquire lock even though prior lease had expired"),
        }
    }

    #[tokio::test]
    async fn test_must_wait_to_acquire() {
        let db = TestDynamoDb::new().await;

        let lock_client = LockClient::new(Arc::new(db.dynamodb_client.clone()));

        let lock_key = "foobar".to_string();
        let alice = "alice".to_string();
        let bob = "bob".to_string();
        let time = 100;

        // 1. Alice acquires lock.
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: alice.clone(),
                lease_duration: Duration::from_millis(time),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        match result.unwrap() {
            AcquireLockResult::Acquired(lock) => {
                assert_eq!(&lock.lease_details().client_id, &alice);
                assert!(!lock.lease_details().is_expired());
            }
            _ => unreachable!("Lock was not acquired!"),
        }

        // 2. Bob attempts to acquire the lock but is rejected.
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: bob.clone(),
                lease_duration: Duration::from_millis(time),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        let prev_lease_details = match result.unwrap() {
            AcquireLockResult::Conflict(Conflict::KnownHolder(lease_details)) => {
                assert_eq!(&lease_details.client_id, &alice);
                assert!(!lease_details.is_expired());
                lease_details
            }
            _ => unreachable!("Bob should have experienced a lock conflict, but he didn't!"),
        };

        // 3. Bob knows that Alice's lease has not yet expired, but he immediately tries again to
        //    acquire the lock anyway. He is rejected.
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: bob.clone(),
                lease_duration: Duration::from_millis(time),
                prev_lease_details: Some(prev_lease_details),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        let prev_lease_details = match result.unwrap() {
            AcquireLockResult::Conflict(Conflict::KnownHolder(lease_details)) => {
                assert_eq!(&lease_details.client_id, &alice);
                assert!(!lease_details.is_expired());
                lease_details
            }
            _ => unreachable!("Bob should have experienced a lock conflict, but he didn't!"),
        };

        // 4. Bob learns his lesson and decides to be patient. He waits until Alice's lease has
        //    expired and tries again to acquire the lock. He succeeds.
        let try_again_at = prev_lease_details
            .lease_started_before
            .checked_add(prev_lease_details.lease_duration)
            .unwrap()
            .checked_add(Duration::from_millis(5)) // wait a little longer for good measure.
            .unwrap();
        let duration_to_wait = try_again_at.duration_since(Instant::now());
        tokio::time::sleep(duration_to_wait).await;
        let result = lock_client
            .try_acquire_lock(&LockOpts {
                lock_key: lock_key.clone(),
                client_id: bob.clone(),
                lease_duration: Duration::from_millis(time),
                prev_lease_details: Some(prev_lease_details),
                ..Default::default()
            })
            .await;
        assert!(result.is_ok());
        match result.unwrap() {
            AcquireLockResult::Acquired(lock) => {
                assert_eq!(&lock.lease_details().client_id, &bob);
                assert!(!lock.lease_details().is_expired());
            }
            _ => unreachable!("Lock was not acquired!"),
        }
    }
}
