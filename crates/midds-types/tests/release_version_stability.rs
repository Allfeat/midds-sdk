//! Wire-format stability for `Release::V1` — mirror of
//! `recording_version_stability.rs`.
//!
//! Pins a known-good `Release::V1` SCALE encoding to disk. Any future change
//! to the V1 byte layout (field reorder, type swap, bound change, …) makes
//! this test fail loudly, forcing a deliberate fixture refresh:
//!
//! ```sh
//! BLESS_VERSION_STABILITY_FIXTURE=1 \
//!   cargo test -p midds-types --test release_version_stability
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
    Country, PartyId, Producer, RecordingRef, Release, ReleaseDate, ReleaseFormat,
    ReleasePackaging, ReleaseStatus, ReleaseType, ReleaseV1, Track,
};
use parity_scale_codec::{Decode, Encode};

const FIXTURE_RELATIVE: &str = "tests/fixtures/release_v1.scale";

fn bv<const N: u32>(s: &[u8]) -> midds_traits::MiddsString<N> {
    BoundedVec::try_from(s.to_vec()).expect("bounded vec build")
}

/// Construct the canonical reference `Release::V1` used by the fixture.
///
/// Hits enough variants to make accidental wire reshuffles visible:
/// - `PartyId::Isni` artist (the longer variant).
/// - A non-empty `featuring` list mixing `PartyId::Ipi` and `PartyId::Both`.
/// - Both `RecordingRef` variants in the tracklist (ISRC then on-chain id),
///   each wrapped in a numbered `Track`.
/// - A populated `Producer` (ISNI + catalog number) pair.
/// - Non-empty `title_aliases` and `cover_contributors`.
/// - A non-`Album` `ReleaseType`, non-`Cd` `format`, non-`None` packaging.
/// - A 13-digit (EAN-13) UPC.
fn reference_v1() -> ReleaseV1 {
    ReleaseV1 {
        upc: bv(b"4006381333931"),
        title: bv(b"Transformer (Deluxe Edition)"),
        title_aliases: BoundedVec::try_from(vec![bv(b"Transformer"), bv(b"Transformeur")])
            .expect("aliases within bound"),
        artist: PartyId::Isni(bv(b"0000000121032683")),
        featuring: BoundedVec::try_from(vec![
            PartyId::Ipi(bv(b"00052210040")),
            PartyId::Both {
                ipi: bv(b"00073050873"),
                isni: bv(b"0000000368571195"),
            },
        ])
        .expect("featuring within bound"),
        tracks: BoundedVec::try_from(vec![
            Track {
                number: 1,
                recording: RecordingRef::Isrc(bv(b"USRC17200312")),
            },
            Track {
                number: 2,
                recording: RecordingRef::Midds(880_017),
            },
        ])
        .expect("tracks within bound"),
        producers: BoundedVec::try_from(vec![Producer {
            isni: bv(b"000000012103268X"),
            catalog_number: bv(b"RCA LSP-4807"),
        }])
        .expect("producers within bound"),
        status: ReleaseStatus::Official,
        release_date: ReleaseDate {
            year: 1972,
            month: 11,
            day: 8,
        },
        country: Country::Us,
        distributor_name: bv(b"RCA Records"),
        release_type: ReleaseType::Album,
        format: ReleaseFormat::Vinyl,
        packaging: ReleasePackaging::Gatefold,
        cover_contributors: BoundedVec::try_from(vec![bv(b"Mick Rock")])
            .expect("cover contributors within bound"),
    }
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_RELATIVE)
}

#[test]
fn reference_payload_passes_validate_format() {
    Release::V1(reference_v1())
        .validate_format()
        .expect("reference payload validates on-chain");
}

/// Wire-format stability: the reference payload's SCALE bytes must equal the
/// committed fixture, and the fixture must decode to the reference payload.
#[test]
fn v1_wire_format_matches_committed_fixture() {
    let reference = Release::V1(reference_v1());
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
         BLESS_VERSION_STABILITY_FIXTURE=1 cargo test -p midds-types --test release_version_stability."
    );

    let decoded = Release::decode(&mut &on_disk[..]).expect("decode committed V1 fixture");
    assert!(
        matches!(decoded, Release::V1(_)),
        "fixture must decode as Release::V1",
    );
    assert_eq!(decoded, reference, "fixture round-trips to reference");
}

/// Identifier stability: the canonical `Upc` extracted from a decoded V1
/// payload must equal the one we constructed. Pins the `Midds::identifier`
/// contract to V1 so a future variant cannot silently rebind it.
#[test]
fn v1_identifier_stays_stable() {
    let reference = Release::V1(reference_v1());
    let encoded = reference.encode();
    let decoded = Release::decode(&mut &encoded[..]).expect("decode reference");
    assert_eq!(decoded.identifier(), reference.identifier());
    assert_eq!(decoded.identifier().as_slice(), b"4006381333931");
}
