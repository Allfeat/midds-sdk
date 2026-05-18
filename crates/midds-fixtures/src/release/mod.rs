//! `Release` fixtures — builder, strategies, corpus, and randomised
//! generation seeded for reproducibility. Sibling of `recording/`; see
//! that module for the rationale behind the `MiddsFixtures` shape.

pub mod builder;
#[cfg(feature = "proptest")]
pub mod strategy;

pub use builder::ReleaseBuilder;

use midds_types::{
    Country, PartyId, Producer, RecordingRef, Release, ReleaseFormat, ReleasePackaging,
    ReleaseStatus, ReleaseType,
};
use rand::Rng;
#[cfg(feature = "corpus")]
use rand::SeedableRng;

use crate::MiddsFixtures;
use crate::identifiers::{ipi_random, isni_random, isrc_random, upc_for_index};

/// `Release` corner of the `MiddsFixtures` trait. Generic test harnesses
/// (mass injection, property tests, CLI bench) drive this struct so adding a
/// new MIDDS type is a matter of supplying a new fixture struct rather than
/// wiring per-type call sites.
pub struct ReleaseFixtures;

impl MiddsFixtures for ReleaseFixtures {
    type Item = Release;

    fn gen_n(seed: u64, count: u32) -> Vec<Self::Item> {
        crate::gen_n_release(seed, count)
    }

    fn pathological() -> Vec<Self::Item> {
        vec![
            crate::pathological::min_size_release(),
            crate::pathological::max_size_release(),
        ]
    }

    fn random_with_index<R: Rng + ?Sized>(rng: &mut R, index: u32) -> Self::Item {
        random_with_upc_index(rng, index)
    }

    fn min_size_with_index(index: u32) -> Self::Item {
        with_upc(crate::pathological::min_size_release(), index)
    }

    fn max_size_with_index(index: u32) -> Self::Item {
        with_upc(crate::pathological::max_size_release(), index)
    }

    #[cfg(feature = "corpus")]
    fn corpus() -> Vec<Self::Item> {
        // No committed full-payload `Release` corpus in V1 — synthesise a
        // small deterministic one so generic harnesses get a non-empty
        // iterator, mirroring `RecordingFixtures::corpus`.
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(0xD15C_0DED_D15C_0DED);
        (0..32u32)
            .map(|i| random_with_upc_index(&mut rng, i))
            .collect()
    }

    #[cfg(feature = "proptest")]
    fn strategy() -> proptest::strategy::BoxedStrategy<Self::Item> {
        use proptest::strategy::Strategy as _;
        crate::release::strategy::arb_release().boxed()
    }
}

/// Swap the canonical UPC of a precomputed release for one derived from
/// `index`. Lets the size-extreme `pathological` payloads carry unique
/// identifiers across a batch without rebuilding the rest of the payload —
/// the `Release` analogue of `recording`'s `with_isrc`.
fn with_upc(release: Release, index: u32) -> Release {
    let Release::V1(mut v1) = release;
    v1.upc = upc_for_index(index);
    Release::V1(v1)
}

/// Build a deterministic `Release` whose UPC is derived from `index`.
///
/// Callers (typically [`crate::gen_n_release`]) supply their own RNG seeded
/// for reproducibility. The UPC comes from `index` so that batches generated
/// with `0..N` always have unique canonical identifiers — `IdentifierIndex`
/// in `pallet-midds` will accept all `N` of them.
pub fn random_with_upc_index<R: Rng + ?Sized>(rng: &mut R, index: u32) -> Release {
    let upc = upc_for_index(index);

    let title_len = rng.gen_range(8..=64usize);
    let title_bytes: Vec<u8> = (0..title_len)
        .map(|_| {
            let alphabet = b"abcdefghijklmnopqrstuvwxyz ";
            alphabet[rng.gen_range(0..alphabet.len())]
        })
        .collect();

    let artist = random_party_id(rng);

    let track_count = rng.gen_range(1..=12usize);
    let tracks: Vec<RecordingRef> = (0..track_count)
        .map(|_| {
            if rng.r#gen::<bool>() {
                RecordingRef::Isrc(isrc_random(rng))
            } else {
                RecordingRef::Midds(rng.r#gen::<u64>())
            }
        })
        .collect();

    let producer_count = rng.gen_range(0..=2usize);
    let producers: Vec<Producer> = (0..producer_count)
        .map(|_| {
            let cat_len = rng.gen_range(1..=12usize);
            let catalog: Vec<u8> = (0..cat_len).map(|_| b'0' + rng.gen_range(0..10)).collect();
            Producer {
                isni: isni_random(rng),
                catalog_number: frame_support::BoundedVec::try_from(catalog)
                    .expect("catalog within bound"),
            }
        })
        .collect();

    let cover_count = rng.gen_range(0..=2usize);
    let covers: Vec<Vec<u8>> = (0..cover_count)
        .map(|_| {
            let n = rng.gen_range(4..=24usize);
            (0..n)
                .map(|_| {
                    let alphabet = b"abcdefghijklmnopqrstuvwxyz ";
                    alphabet[rng.gen_range(0..alphabet.len())]
                })
                .collect()
        })
        .collect();

    let mut b = ReleaseBuilder::new()
        .upc(upc)
        .title(&title_bytes)
        .artist(artist)
        .tracks_unchecked(tracks)
        .producers_unchecked(producers)
        .status(pick_status(rng))
        .release_date(
            rng.gen_range(1900..=2025),
            rng.gen_range(1..=12),
            rng.gen_range(1..=28),
        )
        .country(pick_country(rng))
        .distributor_name(b"Independent Distributor")
        .release_type(pick_type(rng))
        .format(pick_format(rng))
        .packaging(pick_packaging(rng));
    for c in &covers {
        b = b.add_cover_contributor(c);
    }
    b.build()
}

fn random_party_id<R: Rng + ?Sized>(rng: &mut R) -> PartyId {
    if rng.r#gen::<bool>() {
        PartyId::Ipi(ipi_random(rng))
    } else {
        PartyId::Isni(isni_random(rng))
    }
}

fn pick_status<R: Rng + ?Sized>(rng: &mut R) -> ReleaseStatus {
    match rng.gen_range(0..5u8) {
        0 => ReleaseStatus::Official,
        1 => ReleaseStatus::Promotional,
        2 => ReleaseStatus::Bootleg,
        3 => ReleaseStatus::Withdrawn,
        _ => ReleaseStatus::Other,
    }
}

fn pick_type<R: Rng + ?Sized>(rng: &mut R) -> ReleaseType {
    match rng.gen_range(0..6u8) {
        0 => ReleaseType::Album,
        1 => ReleaseType::Single,
        2 => ReleaseType::Ep,
        3 => ReleaseType::Compilation,
        4 => ReleaseType::Live,
        _ => ReleaseType::Other,
    }
}

fn pick_format<R: Rng + ?Sized>(rng: &mut R) -> ReleaseFormat {
    match rng.gen_range(0..6u8) {
        0 => ReleaseFormat::Cd,
        1 => ReleaseFormat::Vinyl,
        2 => ReleaseFormat::Cassette,
        3 => ReleaseFormat::DigitalDownload,
        4 => ReleaseFormat::Streaming,
        _ => ReleaseFormat::Other,
    }
}

fn pick_packaging<R: Rng + ?Sized>(rng: &mut R) -> ReleasePackaging {
    match rng.gen_range(0..5u8) {
        0 => ReleasePackaging::None,
        1 => ReleasePackaging::JewelCase,
        2 => ReleasePackaging::Digipak,
        3 => ReleasePackaging::Gatefold,
        _ => ReleasePackaging::Other,
    }
}

fn pick_country<R: Rng + ?Sized>(rng: &mut R) -> Country {
    match rng.gen_range(0..8u8) {
        0 => Country::Us,
        1 => Country::Fr,
        2 => Country::Gb,
        3 => Country::De,
        4 => Country::Jp,
        5 => Country::Br,
        6 => Country::Au,
        _ => Country::Za,
    }
}
