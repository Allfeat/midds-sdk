//! Helpers for assembling `pallet_utility::batch_all` calls.
//!
//! `subxt::dynamic` does not directly support nested `RuntimeCall` values,
//! so we build the inner calls into raw SCALE bytes (via
//! `TransactionsClient::call_data`) and concatenate them through
//! [`PreEncodedCalls`]. The resulting wire shape is bit-identical to what
//! the runtime would emit for a typed `Vec<RuntimeCall>` — only the static
//! type machinery is bypassed.

use parity_scale_codec::{Compact, Encode, Output};

/// `Vec<RuntimeCall>` whose elements are already in their SCALE wire form
/// (`pallet_index ++ call_index ++ args`).
///
/// Encodes as `Compact(len) ++ concat(inner_bytes)` — the canonical layout
/// for `Vec<T>` in SCALE — letting us push pre-encoded inner calls into
/// `Utility::batch_all` without round-tripping through `scale_value::Value`.
pub struct PreEncodedCalls(Vec<Vec<u8>>);

impl PreEncodedCalls {
    /// Wrap a list of pre-encoded inner-call blobs.
    pub fn new(inner: Vec<Vec<u8>>) -> Self {
        Self(inner)
    }
}

impl Encode for PreEncodedCalls {
    fn encode_to<O: Output + ?Sized>(&self, out: &mut O) {
        Compact(self.0.len() as u32).encode_to(out);
        for inner in &self.0 {
            out.write(inner);
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod tests {
    //! Layout pin for [`PreEncodedCalls`]. The struct is a small, hand-rolled
    //! `Encode` impl that needs to match what `Vec<RuntimeCall>::encode()`
    //! produces when each `RuntimeCall` is already in wire form. Tests below
    //! decode via `parity_scale_codec` so any byte drift surfaces immediately,
    //! without needing a live node.
    use super::*;
    use parity_scale_codec::Decode;

    #[test]
    fn empty_encodes_as_compact_zero() {
        let encoded = PreEncodedCalls::new(vec![]).encode();
        assert_eq!(encoded, Compact(0u32).encode());
    }

    #[test]
    fn round_trip_matches_native_vec_encoding() {
        // Each "call" is opaque bytes — the inner shape is irrelevant to the
        // outer Vec encoding, only the concatenation order matters.
        let inner: Vec<Vec<u8>> = vec![
            vec![0x05, 0x03, 0x01, 0x02],
            vec![0x05, 0x03, 0x09, 0x09, 0x09],
            vec![],
        ];
        let pre = PreEncodedCalls::new(inner.clone()).encode();

        // The native equivalent: SCALE-encoding a `Vec<RawCall>` where
        // `RawCall(Vec<u8>)` writes its bytes verbatim — which is exactly
        // `Compact(len) ++ concat(inners)`.
        let mut native = Vec::new();
        Compact(inner.len() as u32).encode_to(&mut native);
        for v in &inner {
            native.extend_from_slice(v);
        }
        assert_eq!(pre, native);

        // And the leading length prefix decodes cleanly back.
        let mut cursor = &pre[..];
        let len = <Compact<u32>>::decode(&mut cursor).unwrap();
        assert_eq!(len.0 as usize, inner.len());
    }
}
