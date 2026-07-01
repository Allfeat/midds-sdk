//! Wire-format stability for `Recording::V1` — mirror of
//! `version_stability.rs` for `MusicalWork`.
//!
//! Pins a known-good `Recording::V1` SCALE encoding to disk. Any future
//! change to the V1 byte layout (field reorder, type swap, bound change, …)
//! makes this test fail loudly, forcing a deliberate fixture refresh:
//!
//! ```sh
//! BLESS_VERSION_STABILITY_FIXTURE=1 \
//!   cargo test -p midds-types --test recording_version_stability
//! ```
//!
//! The fixture is committed (per `docs/testing.md` §17). A missing fixture
//! is a hard failure — never a silent first-write.
//!
//! The reference payload is constructed by hand (no RNG) so "regenerate the
//! fixture" is reproducible without consulting a seed.

use std::{fs, path::PathBuf};

use bounded_collections::BoundedVec;
use midds_traits::Midds as _;
use midds_types::{
    Genre, Instrument, Mode, MusicalKey, PartyId, Performer, PerformerId, PitchClass,
    ProductionPlaces, Recording, RecordingV1, RecordingVersion, WorkRef,
};
use parity_scale_codec::{Decode, Encode};

const FIXTURE_RELATIVE: &str = "tests/fixtures/recording_v1.scale";

/// Construct the canonical reference `Recording::V1` used by the fixture.
///
/// Hits enough variants to make accidental wire reshuffles visible:
/// - `PartyId::Ipi` for artist, `PartyId::Ipi` for a contributor.
/// - `PerformerId::Ipn` inside a `Performer` carrying two instruments —
///   exercises the performer-specific identifier and the instrument list.
/// - `PartyId::Isni` for a featured artist (non-empty `featuring`).
/// - The string-bearing `WorkRef::Iswc` variant.
/// - `Some(...)` on every `Option` field except `bpm` (kept `None`) — covers
///   both presence and absence in the SCALE Option discriminator; `sub_genre`
///   is `Some` alongside a `Some` primary `genre`.
/// - Non-empty `title_aliases`, `featuring`, `producers`.
/// - `ProductionPlaces` with two of three places populated.
/// - A non-empty offchain extension hash.
fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
    BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
}

fn reference_v1() -> RecordingV1 {
    RecordingV1 {
        isrc: bv(b"FRABC2412345"),
        title: bv(b"Walk on the Wild Side (Live)"),
        title_aliases: BoundedVec::try_from(vec![bv(b"Wild Side"), bv(b"Cote Sauvage")])
            .expect("aliases within bound"),
        artist: PartyId::Ipi(bv(b"123456786")),
        featuring: BoundedVec::try_from(vec![PartyId::Isni(bv(b"0000000121032683"))])
            .expect("featuring within bound"),
        work: WorkRef::Iswc(bv(b"T0345246802")),
        genre: Some(Genre::Rock),
        sub_genre: Some(Genre::Blues),
        record_year: Some(1972),
        version_type: Some(RecordingVersion::Live),
        performers: BoundedVec::try_from(vec![Performer {
            id: PerformerId::Ipn(bv(b"12345678")),
            instruments: BoundedVec::try_from(vec![Instrument::ElectricGuitar, Instrument::Vocals])
                .expect("instruments within bound"),
        }])
        .expect("performers within bound"),
        producers: BoundedVec::try_from(vec![bv(b"000000012103268X")])
            .expect("producers within bound"),
        duration: Some(255),
        bpm: None,
        key: Some(MusicalKey {
            pitch: PitchClass::A,
            mode: Mode::Minor,
        }),
        places: Some(ProductionPlaces {
            recording: Some(bv(b"RCA Studios, New York")),
            mixing: None,
            mastering: Some(bv(b"Sterling Sound")),
        }),
        contributors: BoundedVec::try_from(vec![PartyId::Ipi(bv(b"00000000171"))])
            .expect("contributors within bound"),
        offchain_extension: Some(bv(b"bafkreigh2akiscaildc")),
    }
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_RELATIVE)
}

#[test]
fn reference_payload_passes_validate_format() {
    Recording::V1(reference_v1())
        .validate_format()
        .expect("reference payload validates on-chain");
}

/// Wire-format stability: the reference payload's SCALE bytes must equal the
/// committed fixture, and the fixture must decode to the reference payload.
#[test]
fn v1_wire_format_matches_committed_fixture() {
    let reference = Recording::V1(reference_v1());
    let encoded = reference.encode();
    let path = fixture_path();

    if std::env::var_os("BLESS_VERSION_STABILITY_FIXTURE").is_some() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture dir");
        }
        fs::write(&path, &encoded).expect("write blessed fixture");
        eprintln!("blessed {} ({} bytes)", path.display(), encoded.len());
        return;
    }

    let on_disk = fs::read(&path).unwrap_or_else(|err| {
        panic!(
            "missing fixture {}: {err}. Run with \
             BLESS_VERSION_STABILITY_FIXTURE=1 to create it, then commit.",
            path.display()
        )
    });

    assert_eq!(
        on_disk, encoded,
        "V1 SCALE wire format drifted. If intentional, refresh with \
         BLESS_VERSION_STABILITY_FIXTURE=1 cargo test -p midds-types --test recording_version_stability."
    );

    let decoded = Recording::decode(&mut &on_disk[..]).expect("decode committed V1 fixture");
    assert!(
        matches!(decoded, Recording::V1(_)),
        "fixture must decode as Recording::V1",
    );
    assert_eq!(decoded, reference, "fixture round-trips to reference");
}

/// Identifier stability: the canonical `Isrc` extracted from a decoded V1
/// payload must equal the one we constructed. Pins the `Midds::identifier`
/// contract to V1 so a future variant cannot silently rebind it.
#[test]
fn v1_identifier_stays_stable() {
    let reference = Recording::V1(reference_v1());
    let encoded = reference.encode();
    let decoded = Recording::decode(&mut &encoded[..]).expect("decode reference");
    assert_eq!(decoded.identifier(), reference.identifier());
    assert_eq!(decoded.identifier().as_slice(), b"FRABC2412345");
}
