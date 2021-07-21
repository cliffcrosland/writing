//! Utility functions to generate and parse identifiers.
//!
//! An identifier is a Base-62 encoded UUID. It has a prefix that tells us the type of object it
//! identifies.
//!
//! For example, here is an identifier for a user object:
//! ```
//! "u_8iCDGZ8pK9fAGxySBWh79A"
//! ```
//!
//! And here is one for an organization:
//! ```
//! "o_QCar3LwOwBPIeKonywpCpB"
//! ```
//!
//! We encode UUIDs using Base-62 encoding because it is shorter when written as a string (22 bytes
//! instead of 32 bytes).
//!
//! We try to choose type prefixes to be as short as possible.
//!
//! Type prefixes make it easier to figure out the type of something that appears in the logs.

use std::collections::HashMap;
use std::convert::TryInto;

use base_62::base62;
use enum_iterator::IntoEnumIterator;
use lazy_static::lazy_static;
use uuid::Uuid;

use crate::utils::profanity;

/// The type of object an identifier indentifies.
#[derive(Clone, Copy, Debug, IntoEnumIterator, PartialEq, Eq, Hash)]
pub enum IdType {
    Document,
    LockLease,
    Organization,
    User,
}

/// An identifier with a type. Example:
/// ```
/// let id = Id::new(IdType::User);
/// assert!(id.as_str().starts_with("u_"));
///
/// let id_str = "o_QCar3LwOwBPIeKonywpCpB";
/// let id = Id::parse(id_str);
/// assert!(id.is_some());
/// assert_eq!(id.id_type, IdType::Organization);
/// assert_eq!(id.as_str(), id_str);
/// ```
#[derive(Clone, Debug)]
pub struct Id {
    pub id_type: IdType,
    id_str: String,
}

impl Id {
    /// Generate a new identifier with a type. Example:
    pub fn new(id_type: IdType) -> Self {
        // Rough performance breakdown (in release build):
        //
        // Generate UUID:         20 ns
        // Encode in Base-62:   2800 ns
        // Check for profanity: 1200 ns
        // Format return value:  400 ns
        let mut encoded;
        loop {
            let uuid = Uuid::new_v4();
            encoded = encode_uuid(&uuid);
            // Checking for profanity is extremely fast because we use the Aho-Corasick algorithm.
            // This check is important to do. About 1% of Base-62 encoded uuids contain profanity.
            if !profanity::contains_profanity(&encoded) {
                break;
            }
        }
        let id_str = format!("{}_{}", id_type.as_str(), encoded);
        Self { id_type, id_str }
    }

    /// Parse the id, if we can.
    pub fn parse(id_str: &str) -> Option<Self> {
        let idx = id_str.find('_')?;
        let (prefix, suffix) = id_str.split_at(idx);
        let suffix = &suffix[1..]; // start after '_'

        // valid type prefix, valid Base-62 encoded uuid suffix
        let id_type = IdType::from_str(prefix)?;
        decode_uuid(suffix)?;

        Some(Self {
            id_type,
            id_str: String::from(id_str),
        })
    }

    /// Get the identifier's string.
    pub fn as_str(&self) -> &str {
        &self.id_str
    }
}

impl IdType {
    /// Get the type's prefix. Example:
    /// ```
    /// let t = IdType::User;
    /// assert_eq!(t.as_str(), "u");
    ///
    /// let t = IdType::Document;
    /// assert_eq!(t.as_str(), "d");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match *self {
            IdType::Document => "d",
            IdType::LockLease => "ll",
            IdType::Organization => "o",
            IdType::User => "u",
        }
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
    pub fn from_str(prefix: &str) -> Option<IdType> {
        ID_PREFIX_TO_TYPE.get(prefix).copied()
    }
}

lazy_static! {
    static ref ID_PREFIX_TO_TYPE: HashMap<&'static str, IdType> = IdType::into_enum_iter()
        .map(|id_type| (id_type.as_str(), id_type))
        .collect();
}

/// Encode the UUID in Base-62 encoding.
pub fn encode_uuid(uuid: &Uuid) -> String {
    base62::encode(uuid.as_bytes())
}

/// Decode a UUID from a Base-62 encoded string, if we can.
pub fn decode_uuid(encoded: &str) -> Option<Uuid> {
    let decoded = match base62::decode(encoded) {
        Ok(decoded) => decoded,
        Err(_) => {
            return None;
        }
    };
    let bytes: [u8; 16] = match decoded.try_into() {
        Ok(bytes) => bytes,
        Err(_) => {
            return None;
        }
    };
    Some(Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ids() {
        let user_id = Id::new(IdType::User);
        assert_eq!(user_id.id_type, IdType::User);
        assert!(!user_id.as_str().is_empty());

        let org_id = Id::new(IdType::Organization);
        assert_eq!(org_id.id_type, IdType::Organization);
        assert!(!org_id.as_str().is_empty());

        assert!(Id::parse(user_id.as_str()).is_some());
        assert!(Id::parse(org_id.as_str()).is_some());
    }

    #[test]
    fn test_parse_ids() {
        let invalid_prefix = format!("foobar_{}", encode_uuid(&Uuid::new_v4()));
        assert!(Id::parse(&invalid_prefix).is_none());

        let invalid_suffix = format!("{}_invalidinvalid", IdType::User.as_str());
        assert!(Id::parse(&invalid_suffix).is_none());

        let valid = format!("{}_{}", IdType::User.as_str(), encode_uuid(&Uuid::new_v4()));
        assert!(Id::parse(&valid).is_some());
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
