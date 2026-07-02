//! Custom (de)serializers for `MiddsString<N>`-shaped fields.
//!
//! `MiddsString<N>` is a `BoundedVec<u8, ConstU32<N>>` alias whose default
//! serde shape is a JSON array of bytes (`[84, 49, 50, …]`). MIDDS payloads
//! are ASCII / UTF-8 by convention, so these helpers surface those fields as
//! JSON strings — both for human readability when debugging RPC/API traffic
//! and to give cross-service Rust consumers (explorer, `SaaS`, frontend) a
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
use bounded_collections::{BoundedVec, ConstU32};
use serde::{Deserialize, Deserializer, Serializer, ser::SerializeSeq};

/// Shared serialize-side conversion: the stored bytes as UTF-8 text.
fn as_str<E: serde::ser::Error>(bytes: &[u8]) -> Result<&str, E> {
    core::str::from_utf8(bytes).map_err(E::custom)
}

/// Shared deserialize-side conversion: an owned string into the bounded
/// byte form, surfacing the bound `N` in the error message.
fn bounded_from_string<const N: u32, E: serde::de::Error>(
    s: String,
) -> Result<BoundedVec<u8, ConstU32<N>>, E> {
    let len = s.len();
    BoundedVec::try_from(s.into_bytes())
        .map_err(|_| E::custom(format!("string of {len} bytes exceeds bound {N}")))
}

/// `MiddsString<N>` ↔ JSON string.
///
/// Use as `#[serde(with = "midds_traits::serde_helpers::ascii")]` on any
/// `BoundedVec<u8, ConstU32<N>>` field.
pub mod ascii {
    use super::{
        BoundedVec, ConstU32, Deserialize, Deserializer, Serializer, String, as_str,
        bounded_from_string,
    };

    /// Serializes the bounded bytes as a UTF-8 string.
    pub fn serialize<S, const N: u32>(
        val: &BoundedVec<u8, ConstU32<N>>,
        ser: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        ser.serialize_str(as_str(val.as_slice())?)
    }

    /// Deserializes a string into bounded bytes, rejecting lengths over `N`.
    pub fn deserialize<'de, D, const N: u32>(de: D) -> Result<BoundedVec<u8, ConstU32<N>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        bounded_from_string(String::deserialize(de)?)
    }
}

/// `Option<MiddsString<N>>` ↔ JSON string | null.
///
/// Use as `#[serde(with = "midds_traits::serde_helpers::ascii_opt")]` on any
/// `Option<BoundedVec<u8, ConstU32<N>>>` field.
pub mod ascii_opt {
    use super::{
        BoundedVec, ConstU32, Deserialize, Deserializer, Serializer, String, as_str,
        bounded_from_string,
    };

    /// Serializes the optional bounded bytes as a UTF-8 string or `null`.
    pub fn serialize<S, const N: u32>(
        val: &Option<BoundedVec<u8, ConstU32<N>>>,
        ser: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match val {
            Some(v) => ser.serialize_some(as_str(v.as_slice())?),
            None => ser.serialize_none(),
        }
    }

    /// Deserializes a string or `null` into optional bounded bytes.
    pub fn deserialize<'de, D, const N: u32>(
        de: D,
    ) -> Result<Option<BoundedVec<u8, ConstU32<N>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(de)?
            .map(bounded_from_string)
            .transpose()
    }
}

/// `BoundedVec<MiddsString<N>, ConstU32<M>>` ↔ JSON array of strings.
///
/// Use as `#[serde(with = "midds_traits::serde_helpers::ascii_vec")]` on any
/// `BoundedVec<BoundedVec<u8, ConstU32<N>>, ConstU32<M>>` field (e.g.
/// `WorkReferences`).
pub mod ascii_vec {
    use super::{
        BoundedVec, ConstU32, Deserialize, Deserializer, SerializeSeq, Serializer, String, Vec,
        as_str, bounded_from_string, format,
    };

    /// Serializes each bounded byte string as a UTF-8 string element.
    pub fn serialize<S, const N: u32, const M: u32>(
        val: &BoundedVec<BoundedVec<u8, ConstU32<N>>, ConstU32<M>>,
        ser: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = ser.serialize_seq(Some(val.len()))?;
        for item in val {
            seq.serialize_element(as_str(item.as_slice())?)?;
        }
        seq.end()
    }

    /// Deserializes an array of strings, rejecting inner lengths over `N`
    /// and more than `M` elements.
    pub fn deserialize<'de, D, const N: u32, const M: u32>(
        de: D,
    ) -> Result<BoundedVec<BoundedVec<u8, ConstU32<N>>, ConstU32<M>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Vec<String> = Vec::deserialize(de)?;
        let outer_len = raw.len();
        let inner = raw
            .into_iter()
            .map(bounded_from_string)
            .collect::<Result<Vec<_>, _>>()?;
        BoundedVec::try_from(inner).map_err(|_| {
            serde::de::Error::custom(format!("array of {outer_len} items exceeds bound {M}"))
        })
    }
}
