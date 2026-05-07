//! Custom (de)serializers for `MiddsString<N>`-shaped fields.
//!
//! `MiddsString<N>` is a `BoundedVec<u8, ConstU32<N>>` alias whose default
//! serde shape is a JSON array of bytes (`[84, 49, 50, …]`). MIDDS payloads
//! are ASCII / UTF-8 by convention, so these helpers surface those fields as
//! JSON strings — both for human readability when debugging RPC/API traffic
//! and to give cross-service Rust consumers (explorer, SaaS, frontend) a
//! stable, uniform wire format.
//!
//! These helpers do NOT replicate `validate_*_format` (charset, length,
//! structure). They only enforce the bounded-length cap (`N`); any further
//! validation is the caller's responsibility, so the on-chain format rules
//! remain the single source of truth.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use frame_support::{BoundedVec, traits::ConstU32};
use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};

/// `MiddsString<N>` ↔ JSON string.
///
/// Use as `#[serde(with = "midds_traits::serde_helpers::ascii")]` on any
/// `BoundedVec<u8, ConstU32<N>>` field.
pub mod ascii {
    use super::*;

    pub fn serialize<S, const N: u32>(
        val: &BoundedVec<u8, ConstU32<N>>,
        ser: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = core::str::from_utf8(val.as_slice()).map_err(serde::ser::Error::custom)?;
        ser.serialize_str(s)
    }

    pub fn deserialize<'de, D, const N: u32>(de: D) -> Result<BoundedVec<u8, ConstU32<N>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(de)?;
        let bytes = s.into_bytes();
        let len = bytes.len();
        BoundedVec::try_from(bytes).map_err(|_| {
            serde::de::Error::custom(format!("string of {len} bytes exceeds bound {N}"))
        })
    }
}

/// `Option<MiddsString<N>>` ↔ JSON string | null.
///
/// Use as `#[serde(with = "midds_traits::serde_helpers::ascii_opt")]` on any
/// `Option<BoundedVec<u8, ConstU32<N>>>` field.
pub mod ascii_opt {
    use super::*;

    pub fn serialize<S, const N: u32>(
        val: &Option<BoundedVec<u8, ConstU32<N>>>,
        ser: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match val {
            Some(v) => {
                let s = core::str::from_utf8(v.as_slice()).map_err(serde::ser::Error::custom)?;
                ser.serialize_some(s)
            }
            None => ser.serialize_none(),
        }
    }

    pub fn deserialize<'de, D, const N: u32>(
        de: D,
    ) -> Result<Option<BoundedVec<u8, ConstU32<N>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Option::<String>::deserialize(de)? {
            Some(s) => {
                let bytes = s.into_bytes();
                let len = bytes.len();
                BoundedVec::try_from(bytes).map(Some).map_err(|_| {
                    serde::de::Error::custom(format!("string of {len} bytes exceeds bound {N}"))
                })
            }
            None => Ok(None),
        }
    }
}

/// `BoundedVec<MiddsString<N>, ConstU32<M>>` ↔ JSON array of strings.
///
/// Use as `#[serde(with = "midds_traits::serde_helpers::ascii_vec")]` on any
/// `BoundedVec<BoundedVec<u8, ConstU32<N>>, ConstU32<M>>` field (e.g.
/// `WorkReferences`).
pub mod ascii_vec {
    use super::*;

    pub fn serialize<S, const N: u32, const M: u32>(
        val: &BoundedVec<BoundedVec<u8, ConstU32<N>>, ConstU32<M>>,
        ser: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = ser.serialize_seq(Some(val.len()))?;
        for item in val.iter() {
            let s = core::str::from_utf8(item.as_slice()).map_err(serde::ser::Error::custom)?;
            seq.serialize_element(s)?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D, const N: u32, const M: u32>(
        de: D,
    ) -> Result<BoundedVec<BoundedVec<u8, ConstU32<N>>, ConstU32<M>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Vec<String> = Vec::deserialize(de)?;
        let outer_len = raw.len();
        let mut inner = Vec::with_capacity(outer_len);
        for s in raw {
            let bytes = s.into_bytes();
            let len = bytes.len();
            inner.push(BoundedVec::try_from(bytes).map_err(|_| {
                serde::de::Error::custom(format!("string of {len} bytes exceeds bound {N}"))
            })?);
        }
        BoundedVec::try_from(inner).map_err(|_| {
            serde::de::Error::custom(format!("array of {outer_len} items exceeds bound {M}"))
        })
    }
}
