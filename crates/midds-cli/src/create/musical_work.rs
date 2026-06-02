//! Interactive builder for `MusicalWork::V1`.
//!
//! Each `ui::step` block maps one-to-one onto a `MusicalWorkV1` field group;
//! collection caps and numeric ranges come straight from `midds_types` /
//! `docs/validation.md` so the prompts and the on-chain `validate_format`
//! never disagree.

use anyhow::Result;
use midds_traits::Iswc;
use midds_types::shared::{BPM_MAX, BPM_MIN, YEAR_MAX, YEAR_MIN};
use midds_types::{
    CATALOG_NUMBER_MAX_LEN, CREATORS_MAX, ClassicalInfo, Creator, CreatorRole, CreatorRoles,
    Creators, MusicalWork, MusicalWorkV1, OPUS_MAX_LEN, SAMPLES_MAX, SampleReferences,
    TITLE_MAX_LEN, WORK_REFERENCES_MAX, WorkReferences, WorkType,
};

use crate::create::{prompts, shared};
use crate::ui;

const STEPS: usize = 9;

pub fn build() -> Result<MusicalWork> {
    ui::section("MusicalWork · V1");

    ui::step(1, STEPS, "Identification");
    let iswc: Iswc = prompts::identifier("ISWC", shared::parse_iswc_msg, "T0345246801")?;
    let title = prompts::bounded_string::<TITLE_MAX_LEN>("Title", true)?;

    ui::step(2, STEPS, "Creation");
    let creation_year = prompts::optional_int_in_range::<u16>("Creation year", YEAR_MIN, YEAR_MAX)?;
    let instrumental = prompts::confirm("Instrumental (no lyrics)?", false)?;

    ui::step(3, STEPS, "Language & lyrics");
    let language = prompts::optional("a language", || shared::language("Language"))?;
    // An instrumental work has no lyrics, so `explicit_lyrics` stays false and
    // the prompt is skipped (mirrors the builder-side `instrumental ⇒ …`
    // convention from docs/validation.md §4).
    let explicit_lyrics = if instrumental {
        false
    } else {
        prompts::confirm("Explicit lyrics?", false)?
    };

    ui::step(4, STEPS, "Tempo & key");
    let bpm = prompts::optional_int_in_range::<u16>("BPM", BPM_MIN, BPM_MAX)?;
    let key = prompts::optional("a musical key", || shared::musical_key("Musical key"))?;

    ui::step(5, STEPS, "Work type");
    let work_type = build_work_type()?;

    ui::step(6, STEPS, "Samples");
    let samples = build_samples()?;

    ui::step(7, STEPS, "Creators");
    let creators = build_creators()?;

    ui::step(8, STEPS, "Classical metadata");
    let classical_info = build_classical_info()?;

    ui::step(9, STEPS, "Off-chain extension");
    let offchain_extension = shared::offchain_extension()?;

    Ok(MusicalWork::V1(MusicalWorkV1 {
        iswc,
        title,
        creation_year,
        instrumental,
        language,
        explicit_lyrics,
        bpm,
        key,
        work_type,
        samples,
        creators,
        classical_info,
        offchain_extension,
    }))
}

fn build_work_type() -> Result<WorkType> {
    #[derive(Clone, Copy)]
    enum Kind {
        Original,
        Medley,
        Mashup,
        Adaptation,
        Rearrangement,
    }
    let kind = prompts::select(
        "Work type",
        &[
            ("Original — standalone work", Kind::Original),
            ("Medley — works performed back-to-back", Kind::Medley),
            ("Mashup — works combined into one", Kind::Mashup),
            ("Adaptation — derived from one source", Kind::Adaptation),
            (
                "Rearrangement — re-arranged from one source",
                Kind::Rearrangement,
            ),
        ],
        0,
    )?;
    Ok(match kind {
        Kind::Original => WorkType::Original,
        Kind::Medley | Kind::Mashup => {
            let refs =
                prompts::collect_bounded("source ISWC", 2, WORK_REFERENCES_MAX as usize, |_| {
                    prompts::identifier("Source ISWC", shared::parse_iswc_msg, "T0345246801")
                })?;
            let refs = WorkReferences::try_from(refs)
                .map_err(|_| anyhow::anyhow!("more than {WORK_REFERENCES_MAX} source works"))?;
            if matches!(kind, Kind::Medley) {
                WorkType::Medley(refs)
            } else {
                WorkType::Mashup(refs)
            }
        }
        // Adaptation and Rearrangement both derive from exactly one source.
        Kind::Adaptation | Kind::Rearrangement => {
            let source = prompts::identifier("Source ISWC", shared::parse_iswc_msg, "T0345246801")?;
            if matches!(kind, Kind::Adaptation) {
                WorkType::Adaptation(source)
            } else {
                WorkType::Rearrangement(source)
            }
        }
    })
}

/// Sampled-work references — works this work sampled. Each is a `WorkRef`:
/// an external ISWC, or the cheaper on-chain MIDDS id. The list may be empty.
fn build_samples() -> Result<SampleReferences> {
    let samples = prompts::collect_bounded("sampled work", 0, SAMPLES_MAX as usize, |_| {
        shared::work_ref("Sampled work reference")
    })?;
    SampleReferences::try_from(samples)
        .map_err(|_| anyhow::anyhow!("more than {SAMPLES_MAX} samples"))
}

fn build_creators() -> Result<Creators> {
    let creators = prompts::collect_bounded(
        "creator",
        1,
        CREATORS_MAX as usize,
        |_| -> Result<Creator> {
            let chosen_roles = prompts::multi_select(
                "Roles (space to toggle, enter to confirm — pick at least one)",
                &[
                    ("Author", CreatorRole::Author),
                    ("Composer", CreatorRole::Composer),
                    ("Arranger", CreatorRole::Arranger),
                    ("Adapter", CreatorRole::Adapter),
                    ("Publisher", CreatorRole::Publisher),
                ],
                1,
            )?;
            let mut roles = CreatorRoles::new();
            for r in chosen_roles {
                // multi_select returns each variant at most once (one toggle
                // per menu item), so try_insert can only ever fail on a bound
                // overflow — and there are exactly CREATOR_ROLES_MAX (= 5)
                // menu entries, so the bound holds.
                roles.try_insert(r).expect("role within CREATOR_ROLES_MAX");
            }
            let party = shared::party_id("Creator identifier")?;
            Ok(Creator { roles, party })
        },
    )?;
    Creators::try_from(creators).map_err(|_| anyhow::anyhow!("more than {CREATORS_MAX} creators"))
}

fn build_classical_info() -> Result<Option<ClassicalInfo>> {
    if !prompts::confirm("Add classical metadata (opus / catalogue / voices)?", false)? {
        return Ok(None);
    }
    let opus = prompts::optional_string::<OPUS_MAX_LEN>("Opus (e.g. Op. 27 No. 2)")?;
    let catalog_number =
        prompts::optional_string::<CATALOG_NUMBER_MAX_LEN>("Catalogue number (e.g. BWV 565)")?;
    let number_of_voices = prompts::optional_int_in_range::<u16>("Number of voices", 1, u16::MAX)?;
    Ok(Some(ClassicalInfo {
        opus,
        catalog_number,
        number_of_voices,
    }))
}
