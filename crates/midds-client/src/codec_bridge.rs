//! Bridge between `parity_scale_codec`-encodable MIDDS types and subxt's
//! `EncodeAsFields`-driven dynamic transaction API.
//!
//! On-chain MIDDS types use `BoundedVec` and other Substrate primitives that
//! do not implement `scale_encode::EncodeAsType`. Rather than duplicating the
//! type definitions, we wrap pre-SCALE-encoded field bytes in [`EncodedCall`]
//! and pass them through the dynamic call path. The byte layout still matches
//! what the runtime decodes — `BoundedVec<u8, N>` and `Vec<u8>` share an
//! identical SCALE encoding, etc. — so this is a thin shim, not a workaround.

use parity_scale_codec::Encode;
use subxt::ext::scale_encode::{EncodeAsFields, Error as EncodeError, FieldIter, TypeResolver};

/// Pre-encoded call arguments for a dispatchable.
///
/// Each entry in [`EncodedCall::fields`] holds the canonical SCALE encoding
/// of one call argument, in the order expected by the pallet.
pub struct EncodedCall {
    fields: Vec<Vec<u8>>,
}

impl EncodedCall {
    /// Single-field call (e.g. `deposit(item)`).
    pub fn one<A: Encode>(a: &A) -> Self {
        Self {
            fields: vec![a.encode()],
        }
    }

    /// Two-field call (e.g. `update(id, item)`).
    pub fn two<A: Encode, B: Encode>(a: &A, b: &B) -> Self {
        Self {
            fields: vec![a.encode(), b.encode()],
        }
    }
}

impl EncodeAsFields for EncodedCall {
    fn encode_as_fields_to<R: TypeResolver>(
        &self,
        fields: &mut dyn FieldIter<'_, R::TypeId>,
        _types: &R,
        out: &mut Vec<u8>,
    ) -> Result<(), EncodeError> {
        for (idx, blob) in self.fields.iter().enumerate() {
            if fields.next().is_none() {
                return Err(EncodeError::custom_string(format!(
                    "midds-client: metadata exposed only {idx} fields, expected {}",
                    self.fields.len(),
                )));
            }
            out.extend_from_slice(blob);
        }
        if fields.next().is_some() {
            return Err(EncodeError::custom_str(
                "midds-client: metadata expected more fields than provided",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! The tests below run against the real `EncodeAsFields` glue rather than
    //! poking at the private `fields` slice — that's the only way to catch a
    //! regression in the field/blob-iteration logic, which is what subxt
    //! actually invokes at submission time. A nop `TypeResolver` is enough
    //! because `EncodedCall` never inspects type info; the only thing that
    //! matters here is the field-count accounting and the byte layout of the
    //! produced output buffer.
    use super::*;
    use parity_scale_codec::Encode;
    use subxt::ext::scale_encode::EncodeAsFields;
    use subxt::ext::scale_type_resolver::{Field, ResolvedTypeVisitor, TypeResolver};

    /// `TypeResolver` that never resolves a type — `EncodedCall` only iterates
    /// fields, so the resolver is a phantom. Using a `Field<()>` keeps the
    /// `TypeId` parameter trivially constructible in test fields.
    struct NopResolver;
    impl TypeResolver for NopResolver {
        type TypeId = ();
        type Error = core::convert::Infallible;
        fn resolve_type<'this, V: ResolvedTypeVisitor<'this, TypeId = Self::TypeId>>(
            &'this self,
            _type_id: Self::TypeId,
            _visitor: V,
        ) -> Result<V::Value, Self::Error> {
            unreachable!("EncodedCall never resolves types")
        }
    }

    /// Build `n` unnamed fields with the unit `TypeId` — matches the iterator
    /// shape `subxt::dynamic::tx` would feed in production.
    fn fields(n: usize) -> Vec<Field<'static, ()>> {
        (0..n).map(|_| Field::unnamed(())).collect()
    }

    fn run<X: EncodeAsFields>(call: &X, field_count: usize) -> Result<Vec<u8>, EncodeError> {
        let resolver = NopResolver;
        let fields_vec = fields(field_count);
        let mut iter = fields_vec.into_iter();
        let mut out = Vec::new();
        call.encode_as_fields_to(&mut iter, &resolver, &mut out)?;
        Ok(out)
    }

    #[test]
    fn one_field_emits_inner_encoding_verbatim() {
        let value: u64 = 0xDEAD_BEEF_CAFE_F00D;
        let call = EncodedCall::one(&value);
        let bytes = run(&call, 1).expect("one-field call encodes");
        assert_eq!(bytes, value.encode());
    }

    #[test]
    fn two_fields_concatenate_encodings_in_order() {
        let id: u64 = 42;
        let payload: Vec<u8> = vec![1, 2, 3, 4, 5];
        let call = EncodedCall::two(&id, &payload);
        let bytes = run(&call, 2).expect("two-field call encodes");

        let mut expected = Vec::new();
        expected.extend(id.encode());
        expected.extend(payload.encode());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn fewer_fields_than_blobs_errors() {
        // Caller supplied two encoded blobs but the metadata only advertises
        // one field. Catches the runtime-side mismatch where someone changes a
        // dispatchable signature without bumping `EncodedCall::two` to ::one.
        let call = EncodedCall::two(&1u32, &2u32);
        let err = run(&call, 1).expect_err("must reject narrower metadata");
        let msg = err.to_string();
        assert!(
            msg.contains("metadata exposed only 1 fields"),
            "unexpected error: {msg}",
        );
    }

    #[test]
    fn more_fields_than_blobs_errors() {
        // Reverse: metadata advertises two fields but the caller only encoded
        // one. Catches the case where a dispatchable gains a new argument.
        let call = EncodedCall::one(&1u32);
        let err = run(&call, 2).expect_err("must reject wider metadata");
        let msg = err.to_string();
        assert!(
            msg.contains("metadata expected more fields than provided"),
            "unexpected error: {msg}",
        );
    }

    /// Roundtrip via `parity_scale_codec` on a non-trivial type:
    /// `EncodedCall::one(&x)` must produce bytes that decode back to `x`. Pins
    /// the bridge invariant (`bridge bytes == x.encode()`) on a value with
    /// length-prefixed fields, which is what guards against silent layout
    /// drift on `BoundedVec` / `Option`.
    #[test]
    fn parity_codec_roundtrip_through_bridge() {
        use parity_scale_codec::Decode;

        #[derive(Encode, Decode, PartialEq, Eq, Debug, Clone)]
        struct Sample {
            a: u32,
            b: Vec<u8>,
            c: Option<u64>,
        }
        let value = Sample {
            a: 7,
            b: b"midds".to_vec(),
            c: Some(0x00C0_FFEE),
        };
        let call = EncodedCall::one(&value);
        let bytes = run(&call, 1).expect("encode");
        let decoded = Sample::decode(&mut &bytes[..]).expect("decode");
        assert_eq!(decoded, value);
    }
}
