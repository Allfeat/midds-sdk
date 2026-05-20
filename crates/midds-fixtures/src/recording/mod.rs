//! `Recording` fixtures — builder, strategies, corpus, and randomised
//! generation seeded for reproducibility. Sibling of `musical_work/`; see
//! that module for the rationale behind the `MiddsFixtures` shape.

pub mod builder;
#[cfg(feature = "proptest")]
pub mod strategy;

pub use builder::RecordingBuilder;

use midds_types::{
    Genre, Mode, MusicalKey, PartyId, PitchClass, Recording, RecordingVersion, WorkRef,
};
use rand::Rng;
#[cfg(feature = "corpus")]
use rand::SeedableRng;

use crate::MiddsFixtures;
use crate::identifiers::{ipi_random, isni_random, isrc_for_index, iswc_from_work_code};

/// `Recording` corner of the `MiddsFixtures` trait. Generic test harnesses
/// (mass injection, property tests, CLI bench) drive this struct so adding a
/// new MIDDS type is a matter of supplying a new fixture struct rather than
/// wiring per-type call sites.
pub struct RecordingFixtures;

impl MiddsFixtures for RecordingFixtures {
    type Item = Recording;

    fn gen_n(seed: u64, count: u32) -> Vec<Self::Item> {
        crate::gen_n_recording(seed, count)
    }

    fn pathological() -> Vec<Self::Item> {
        vec![
            crate::pathological::min_size_recording(),
            crate::pathological::max_size_recording(),
        ]
    }

    fn random_with_index<R: Rng + ?Sized>(rng: &mut R, index: u32) -> Self::Item {
        random_with_isrc_index(rng, index)
    }

    fn min_size_with_index(index: u32) -> Self::Item {
        with_isrc(crate::pathological::min_size_recording(), index)
    }

    fn max_size_with_index(index: u32) -> Self::Item {
        with_isrc(crate::pathological::max_size_recording(), index)
    }

    #[cfg(feature = "corpus")]
    fn corpus() -> Vec<Self::Item> {
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(0xC0DE_C0DE_C0DE_C0DE);
        (0..32u32)
            .map(|i| random_with_isrc_index(&mut rng, i))
            .collect()
    }

    #[cfg(feature = "proptest")]
    fn strategy() -> proptest::strategy::BoxedStrategy<Self::Item> {
        use proptest::strategy::Strategy as _;
        crate::recording::strategy::arb_recording().boxed()
    }
}

/// Swap the canonical ISRC of a precomputed recording for one derived from
/// `index`. Lets the size-extreme `pathological` payloads carry unique
/// identifiers across a batch without rebuilding the rest of the payload —
/// the `Recording` analogue of `musical_work`'s `with_iswc`.
fn with_isrc(recording: Recording, index: u32) -> Recording {
    let Recording::V1(mut v1) = recording;
    v1.isrc = isrc_for_index(index);
    Recording::V1(v1)
}

/// Build a deterministic `Recording` whose ISRC is derived from `index`.
///
/// Callers (typically [`crate::gen_n_recording`]) supply their own RNG seeded
/// for reproducibility. The ISRC comes from `index` so that batches generated
/// with `0..N` always have unique canonical identifiers — `IdentifierIndex`
/// in `pallet-midds` will accept all `N` of them.
pub fn random_with_isrc_index<R: Rng + ?Sized>(rng: &mut R, index: u32) -> Recording {
    let isrc = isrc_for_index(index);

    let title_len = rng.gen_range(8..=64usize);
    let title_bytes: Vec<u8> = (0..title_len)
        .map(|_| {
            let alphabet = b"abcdefghijklmnopqrstuvwxyz ";
            alphabet[rng.gen_range(0..alphabet.len())]
        })
        .collect();

    let artist = random_party_id(rng);

    let work = if rng.r#gen::<bool>() {
        WorkRef::Iswc(iswc_from_work_code(rng.r#gen::<u32>()))
    } else {
        WorkRef::Midds(rng.r#gen::<u64>())
    };

    let genre_count = rng.gen_range(0..=3usize);
    let genres: Vec<Genre> = (0..genre_count).map(|_| pick_genre(rng)).collect();

    let performer_count = rng.gen_range(0..=3usize);
    let performers: Vec<PartyId> = (0..performer_count).map(|_| random_party_id(rng)).collect();

    let producer_count = rng.gen_range(0..=2usize);
    let producers: Vec<midds_traits::Isni> =
        (0..producer_count).map(|_| isni_random(rng)).collect();

    let contributor_count = rng.gen_range(0..=2usize);
    let contributors: Vec<PartyId> = (0..contributor_count)
        .map(|_| random_party_id(rng))
        .collect();

    RecordingBuilder::new()
        .isrc(isrc)
        .title(&title_bytes)
        .artist(artist)
        .work(work)
        .genres_unchecked(genres)
        .record_year(rng.gen_range(1900..=2025))
        .version_type(pick_version(rng))
        .performers_unchecked(performers)
        .producers_unchecked(producers)
        .duration(rng.gen_range(30..=600))
        .bpm(rng.gen_range(40..=240))
        .key(MusicalKey {
            pitch: PitchClass::C,
            mode: Mode::Major,
        })
        .contributors_unchecked(contributors)
        .build()
}

fn random_party_id<R: Rng + ?Sized>(rng: &mut R) -> PartyId {
    if rng.r#gen::<bool>() {
        PartyId::Ipi(ipi_random(rng))
    } else {
        PartyId::Isni(isni_random(rng))
    }
}

fn pick_genre<R: Rng + ?Sized>(rng: &mut R) -> Genre {
    match rng.gen_range(0..10u8) {
        0 => Genre::Pop,
        1 => Genre::Rock,
        2 => Genre::HipHop,
        3 => Genre::RnB,
        4 => Genre::Electronic,
        5 => Genre::Jazz,
        6 => Genre::Classical,
        7 => Genre::Folk,
        8 => Genre::Soul,
        _ => Genre::Other,
    }
}

fn pick_version<R: Rng + ?Sized>(rng: &mut R) -> RecordingVersion {
    match rng.gen_range(0..6u8) {
        0 => RecordingVersion::Original,
        1 => RecordingVersion::RadioEdit,
        2 => RecordingVersion::Extended,
        3 => RecordingVersion::Live,
        4 => RecordingVersion::Acoustic,
        _ => RecordingVersion::Remix,
    }
}
