//! Utility functions to generate and parse identifiers.
//!
//! An identifier is a Base-62 encoded UUID. It has a prefix that tells us the type of object it
//! identifies.
//!
//! For example, here is an identifier for a user object:
//! ```
//! "u_ZV5pzlG1jWxjwtDk4WPynA8AK8"
//! ```
//!
//! And here is one for an organization:
//! ```
//! "o_mYPvQKxLOYjoFPOk8D48jzk7J"
//! ```
//!
//! We encode UUIDs using Base-62 encoding because it makes a UUID shorter when it is written as a
//! string (25-26 bytes instead of 32 bytes).
//!
//! We try to choose type prefixes to be as short as possible.
//!
//! Type prefixes make it easier to figure out the type of something that appears in the logs.

use std::collections::HashMap;

use enum_iterator::IntoEnumIterator;
use hash_ids::HashIds;
use lazy_static::lazy_static;
use uuid::Uuid;

/// The type of object an identifier indentifies.
#[derive(Clone, Copy, Debug, IntoEnumIterator, PartialEq, Eq, Hash)]
pub enum IdType {
    User,
    Organization,
    Page,
    LockLease,
}

lazy_static! {
    static ref HASH_IDS: HashIds = HashIds::builder().finish();
    static ref ID_PREFIX_TO_TYPE: HashMap<&'static str, IdType> = [
        ("u", IdType::User),
        ("o", IdType::Organization),
        ("p", IdType::Page),
        ("ll", IdType::LockLease),
    ]
    .iter()
    .copied()
    .collect();
    static ref ID_TYPE_TO_PREFIX: HashMap<IdType, &'static str> = ID_PREFIX_TO_TYPE
        .iter()
        .map(|(prefix, id_type)| (*id_type, *prefix))
        .collect();
}

impl IdType {
    /// Get the type's prefix. Example:
    /// ```
    /// let t = IdType::User;
    /// assert_eq!(t.as_str(), "u");
    ///
    /// let t = IdType::Page;
    /// assert_eq!(t.as_str(), "p");
    /// ```
    pub fn as_str(&self) -> &str {
        ID_TYPE_TO_PREFIX
            .get(&self)
            .expect("Incomplete id type registry!")
    }

    /// Parse the type prefix and get the type, if it exists. Example:
    /// ```
    /// let t = IdType::from_str("u");
    /// assert!(t.is_some());
    /// assert_eq!(t.unwrap(), IdType::User);
    ///
    /// let t = IdType::from_str("foobar");
    /// assert!(t.is_none());
    /// ```
    pub fn from_str(s: &str) -> Option<IdType> {
        ID_PREFIX_TO_TYPE.get(s).copied()
    }
}

/// Generates a new id for the given `IdType`. Example:
/// ```
/// let id = new_id(IdType::User);
/// assert!(id.starts_with("u_"));
/// ```
pub fn new_id(id_type: IdType) -> String {
    let uuid = Uuid::new_v4();
    let id_str = encode_uuid(&uuid);
    format!("{}_{}", id_type.as_str(), &id_str)
}

/// Get the type for a given id. We look at the prefix to find it. Example:
/// ```
/// let id = "u_ZV5pzlG1jWxjwtDk4WPynA8AK8";
/// let t = get_id_type(id);
/// assert!(t.is_some());
/// assert_eq!(t.unwrap(), IdType::User);
/// ```
pub fn get_id_type(id_str: &str) -> Option<IdType> {
    match id_str.find('_') {
        Some(idx) => IdType::from_str(&id_str[0..idx]),
        None => None,
    }
}

const LOWER_HALF_MASK: u128 = (1u128 << 64) - 1;

/// Encode the UUID in Base-62 encoding.
pub fn encode_uuid(uuid: &Uuid) -> String {
    let num = uuid.as_u128();
    let upper_half = (num >> 64) as u64;
    let lower_half = (num & LOWER_HALF_MASK) as u64;
    let halves: [u64; 2] = [upper_half, lower_half];
    HASH_IDS.encode(&halves)
}

/// Decode a UUID from a Base-62 encoded string, if we can.
pub fn decode_uuid(encoded: &str) -> Option<Uuid> {
    let halves = HASH_IDS.decode(encoded);
    if halves.len() != 2 {
        return None;
    }
    assert!(halves.len() == 2);
    let num = ((halves[0] as u128) << 64) | (halves[1] as u128);
    Some(Uuid::from_u128(num))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_id_types_supported() {
        // Test that all `IdType` enum variants appear in the prefix map.
        for id_type in IdType::into_enum_iter() {
            let s = id_type.as_str();
            let found_id_type = IdType::from_str(s);
            assert!(found_id_type.is_some());
            let found_id_type = found_id_type.unwrap();
            assert_eq!(id_type, found_id_type);
        }
    }

    #[test]
    fn test_generate_ids() {
        let user_id = new_id(IdType::User);
        assert!(!user_id.is_empty());
        let id_type = get_id_type(&user_id);
        assert!(id_type.is_some());
        let id_type = id_type.unwrap();
        assert_eq!(id_type, IdType::User);

        let org_id = new_id(IdType::Organization);
        assert!(!org_id.is_empty());
        let id_type = get_id_type(&org_id);
        assert!(id_type.is_some());
        let id_type = id_type.unwrap();
        assert_eq!(id_type, IdType::Organization);

        assert_ne!(user_id, org_id);
    }

    #[test]
    fn test_encode_and_decode_uuid() {
        let id = Uuid::new_v4();
        let encoded = encode_uuid(&id);
        assert!(encoded.len() < id.to_hyphenated().to_string().len());
        let decoded_id = decode_uuid(&encoded);
        assert!(decoded_id.is_some());
        assert_eq!(decoded_id.unwrap(), id);
    }
}
