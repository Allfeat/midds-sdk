//! Wire-format stability for `MusicalWork::V1` — section 16 of
//! `docs/testing.md`.
//!
//! Pins a known-good `MusicalWork::V1` SCALE encoding to disk. Any future
//! change to the V1 byte layout (field reorder, type swap, bound change, …)
//! makes this test fail loudly, forcing a deliberate fixture refresh:
//!
//! ```sh
//! BLESS_VERSION_STABILITY_FIXTURE=1 \
//!   cargo test -p midds-types --test version_stability
//! ```
//!
//! The repo policy (per `docs/testing.md` §17) is to **always** commit this
//! fixture. A missing fixture is a hard failure — never a silent first-write
//! — so CI cannot accidentally bless an unreviewed encoding.
//!
//! The reference payload is constructed by hand (no `arb_musical_work()` and
//! no RNG) so that "regenerate the fixture" is reproducible without consulting
//! a seed. If the payload itself ever needs to evolve, treat that as a
//! migration moment: bless and document why in the same PR.

use std::{fs, path::PathBuf};

use bounded_collections::BoundedVec;
use midds_traits::Midds as _;
use midds_types::{
    ClassicalInfo, Creator, CreatorId, CreatorRole, Language, Mode, MusicalKey, MusicalWork,
    MusicalWorkV1, PitchClass, WorkType,
};
use parity_scale_codec::{Decode, Encode};

const FIXTURE_RELATIVE: &str = "tests/fixtures/musical_work_v1.scale";

/// Construct the canonical reference `MusicalWork::V1` used by the fixture.
///
/// Hits enough variants to make accidental wire reshuffles visible:
/// - Both `CreatorId` variants (IPI + ISNI).
/// - A non-`Original` `WorkType` (Adaptation references one ISWC).
/// - `Some(...)` on every `Option` field except `bpm` (kept `None`) — covers
///   both presence and absence in the SCALE Option discriminator.
/// - `ClassicalInfo` populated with both string fields plus `number_of_voices`.
/// - A non-empty offchain extension hash.
fn reference_v1() -> MusicalWorkV1 {
    let iswc = BoundedVec::try_from(b"T0345246802".to_vec()).expect("11-byte ISWC");
    let title = BoundedVec::try_from(b"Walk on the Wild Side".to_vec()).expect("title bound");
    let composer = Creator {
        role: CreatorRole::Composer,
        id: CreatorId::Ipi(BoundedVec::try_from(b"123456786".to_vec()).expect("9-byte IPI")),
    };
    let author = Creator {
        role: CreatorRole::Author,
        id: CreatorId::Isni(
            BoundedVec::try_from(b"0000000121032683".to_vec()).expect("16-byte ISNI"),
        ),
    };
    let creators =
        BoundedVec::try_from(vec![composer, author]).expect("2 creators within CREATORS_MAX");
    let classical_info = Some(ClassicalInfo {
        opus: Some(BoundedVec::try_from(b"Op. 27 No. 2".to_vec()).expect("opus bound")),
        catalog_number: Some(BoundedVec::try_from(b"K. 545".to_vec()).expect("catalog bound")),
        number_of_voices: Some(4),
    });
    let offchain_extension =
        Some(BoundedVec::try_from(b"bafkreigh2akiscaildc".to_vec()).expect("offchain hash bound"));

    MusicalWorkV1 {
        iswc,
        title,
        creation_year: 1972,
        instrumental: false,
        language: Some(Language::En),
        bpm: None,
        key: Some(MusicalKey {
            pitch: PitchClass::A,
            mode: Mode::Minor,
        }),
        work_type: WorkType::Adaptation(
            BoundedVec::try_from(b"T9876543210".to_vec()).expect("11-byte adaptation ISWC"),
        ),
        creators,
        classical_info,
        offchain_extension,
    }
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_RELATIVE)
}

#[test]
fn reference_payload_passes_validate_format() {
    MusicalWork::V1(reference_v1())
        .validate_format()
        .expect("reference payload validates on-chain");
}

/// Wire-format stability: the reference payload's SCALE bytes must equal the
/// committed fixture, and the fixture must decode to the reference payload.
#[test]
fn v1_wire_format_matches_committed_fixture() {
    let reference = MusicalWork::V1(reference_v1());
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
         BLESS_VERSION_STABILITY_FIXTURE=1 cargo test -p midds-types --test version_stability."
    );

    let decoded = MusicalWork::decode(&mut &on_disk[..]).expect("decode committed V1 fixture");
    assert!(
        matches!(decoded, MusicalWork::V1(_)),
        "fixture must decode as MusicalWork::V1",
    );
    assert_eq!(decoded, reference, "fixture round-trips to reference");
}

/// Identifier stability: the canonical `Iswc` extracted from a decoded V1
/// payload must equal the one we constructed. Pins the `Midds::identifier`
/// contract to V1 so a future variant cannot silently rebind the canonical
/// identifier accessor.
#[test]
fn v1_identifier_stays_stable() {
    let reference = MusicalWork::V1(reference_v1());
    let encoded = reference.encode();
    let decoded = MusicalWork::decode(&mut &encoded[..]).expect("decode reference");
    assert_eq!(decoded.identifier(), reference.identifier());
    assert_eq!(decoded.identifier().as_slice(), b"T0345246802");
}
