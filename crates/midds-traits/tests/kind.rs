//! Pin the `Midds::KIND` discriminator so future renames surface as test
//! breakage. `KIND` is wire-visible (events, RPC namespaces, indexer
//! schemas), so a silent rename would silently break consumers.

use midds_traits::{Iswc, Midds, MiddsFormatError, MiddsString};
use parity_scale_codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use scale_info::TypeInfo;

#[derive(
    Encode, Decode, DecodeWithMemTracking, TypeInfo, MaxEncodedLen, Clone, PartialEq, Eq, Debug,
)]
struct DummyMusicalWork(MiddsString<11>);

impl Midds for DummyMusicalWork {
    const KIND: &'static str = "MusicalWork";
    type Identifier = Iswc;
    fn identifier(&self) -> &Iswc {
        &self.0
    }
    fn validate_format(&self) -> Result<(), MiddsFormatError> {
        Ok(())
    }
}

#[test]
fn musical_work_kind_is_pascal_case_singular() {
    assert_eq!(DummyMusicalWork::KIND, "MusicalWork");
}
