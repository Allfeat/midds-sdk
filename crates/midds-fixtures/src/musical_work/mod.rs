//! `MusicalWork` fixtures — builder, strategies, corpus, and randomised
//! generation seeded for reproducibility.

pub mod builder;
#[cfg(feature = "corpus")]
pub mod corpus;
#[cfg(feature = "proptest")]
pub mod strategy;

pub use builder::MusicalWorkBuilder;

use midds_types::{
    Creator, CreatorId, CreatorRole, Language, Mode, MusicalKey, MusicalWork, PitchClass, WorkType,
};
use rand::Rng;
#[cfg(feature = "corpus")]
use rand::SeedableRng;

use crate::MiddsFixtures;
use crate::identifiers::{ipi_random, isni_random, iswc_for_index};

/// `MusicalWork` corner of the `MiddsFixtures` trait. Generic test
/// harnesses (mass injection, property tests, CLI bench) drive this struct
/// instead of calling `gen_n` / `pathological_*` free functions, so adding
/// a new MIDDS type is a matter of supplying a new fixture struct rather
/// than wiring per-type call sites.
pub struct MusicalWorkFixtures;

impl MiddsFixtures for MusicalWorkFixtures {
    type Item = MusicalWork;

    fn gen_n(seed: u64, count: u32) -> Vec<Self::Item> {
        crate::gen_n(seed, count)
    }

    fn pathological() -> Vec<Self::Item> {
        vec![
            crate::pathological::min_size_musical_work(),
            crate::pathological::max_size_musical_work(),
        ]
    }

    #[cfg(feature = "corpus")]
    fn corpus() -> Vec<Self::Item> {
        // V1 ships per-identifier corpora (ISWC / IPI / ISNI in
        // `data/*.json`) but no full-payload `MusicalWork` corpus — synthesise
        // a small, deterministic one so generic harnesses get something
        // non-empty to iterate over.
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(0xC0DE_C0DE_C0DE_C0DE);
        (0..32u32)
            .map(|i| random_with_iswc_index(&mut rng, i))
            .collect()
    }

    #[cfg(feature = "proptest")]
    fn strategy() -> proptest::strategy::BoxedStrategy<Self::Item> {
        use proptest::strategy::Strategy as _;
        crate::musical_work::strategy::arb_musical_work().boxed()
    }
}

/// Build a deterministic `MusicalWork` whose ISWC is derived from `index`.
///
/// Callers (typically [`crate::gen_n`]) supply their own RNG seeded for
/// reproducibility. The ISWC comes from `index` so that batches generated
/// with `0..N` always have unique canonical identifiers — `IdentifierIndex`
/// in `pallet-midds` will accept all `N` of them.
pub fn random_with_iswc_index<R: Rng + ?Sized>(rng: &mut R, index: u32) -> MusicalWork {
    let iswc = iswc_for_index(index);

    let title_len = rng.gen_range(8..=64usize);
    let title_bytes: Vec<u8> = (0..title_len)
        .map(|_| {
            let alphabet = b"abcdefghijklmnopqrstuvwxyz ";
            alphabet[rng.gen_range(0..alphabet.len())]
        })
        .collect();

    let creators_count = rng.gen_range(1..=3usize);
    let mut creators = Vec::with_capacity(creators_count);
    for _ in 0..creators_count {
        let role = pick_role(rng);
        let id = if rng.r#gen::<bool>() {
            CreatorId::Ipi(ipi_random(rng))
        } else {
            CreatorId::Isni(isni_random(rng))
        };
        creators.push(Creator { role, id });
    }

    MusicalWorkBuilder::new()
        .iswc(iswc)
        .title(&title_bytes)
        .creation_year(rng.gen_range(1900..=2025))
        .instrumental(false)
        .language(Language::En)
        .bpm(rng.gen_range(40..=240))
        .key(MusicalKey {
            pitch: PitchClass::C,
            mode: Mode::Major,
        })
        .work_type(WorkType::Original)
        .creators_unchecked(creators)
        .build()
}

fn pick_role<R: Rng + ?Sized>(rng: &mut R) -> CreatorRole {
    match rng.gen_range(0..5u8) {
        0 => CreatorRole::Author,
        1 => CreatorRole::Composer,
        2 => CreatorRole::Arranger,
        3 => CreatorRole::Adapter,
        _ => CreatorRole::Publisher,
    }
}
