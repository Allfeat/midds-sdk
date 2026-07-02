//! Generator / strategy helpers shared by the per-MIDDS modules. Changing
//! RNG call order/shape here breaks the committed storage-root fixtures.

use midds_types::PartyId;
use rand::Rng;

use crate::identifiers::{ipi_random, isni_random};

/// Random [`PartyId`] drawing IPI or ISNI with a fair coin — shared by the
/// Recording and Release `random_with_index` generators.
pub(crate) fn random_party_id<R: Rng + ?Sized>(rng: &mut R) -> PartyId {
    if rng.r#gen::<bool>() {
        PartyId::Ipi(ipi_random(rng))
    } else {
        PartyId::Isni(isni_random(rng))
    }
}

/// Strategy over the printable ASCII range — the charset every free-text
/// MIDDS field strategy draws from.
#[cfg(feature = "proptest")]
pub(crate) fn printable_ascii() -> impl proptest::strategy::Strategy<Value = u8> {
    32u8..=126u8
}
