//! Interactive builder for `MusicalWork::V1`.
//!
//! Each `ui::step` block maps one-to-one onto a `MusicalWorkV1` field group;
//! collection caps and numeric ranges come straight from `midds_types` /
//! `docs/validation.md` so the prompts and the on-chain `validate_format`
//! never disagree.

use anyhow::Result;
use midds_traits::{Iswc, validate_iswc_format, validate_offchain_hash};
use midds_types::shared::{BPM_MAX, BPM_MIN, YEAR_MAX, YEAR_MIN};
use midds_types::{
    CATALOG_NUMBER_MAX_LEN, CREATORS_MAX, ClassicalInfo, Creator, CreatorId, CreatorRole, Creators,
    MusicalWork, MusicalWorkV1, OPUS_MAX_LEN, TITLE_MAX_LEN, WORK_REFERENCES_MAX, WorkReferences,
    WorkType,
};

use crate::create::{prompts, shared};
use crate::ui;

const STEPS: usize = 9;

pub fn build() -> Result<MusicalWork> {
    ui::section("MusicalWork · V1");

    ui::step(1, STEPS, "Identification");
    let iswc: Iswc = prompts::identifier::<11>("ISWC", validate_iswc_format, "T0345246801")?;
    let title = prompts::bounded_string::<TITLE_MAX_LEN>("Title", true)?;

    ui::step(2, STEPS, "Creation");
    let creation_year = prompts::int_in_range::<u16>("Creation year", YEAR_MIN, YEAR_MAX, None)?;
    let instrumental = prompts::confirm("Instrumental (no lyrics)?", false)?;

    ui::step(3, STEPS, "Language");
    let language = if prompts::confirm("Set a language?", false)? {
        Some(shared::language("Language")?)
    } else {
        None
    };

    ui::step(4, STEPS, "Tempo & key");
    let bpm = prompts::optional_int_in_range::<u16>("BPM", BPM_MIN, BPM_MAX)?;
    let key = if prompts::confirm("Set a musical key?", false)? {
        Some(shared::musical_key("Musical key")?)
    } else {
        None
    };

    ui::step(5, STEPS, "Work type");
    let work_type = build_work_type()?;

    ui::step(6, STEPS, "Creators");
    let creators = build_creators()?;

    ui::step(7, STEPS, "Classical metadata");
    let classical_info = build_classical_info()?;

    ui::step(8, STEPS, "Off-chain extension");
    let offchain_extension = if prompts::confirm("Attach an off-chain extension hash?", false)? {
        Some(prompts::identifier::<64>(
            "Off-chain hash",
            validate_offchain_hash,
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
        )?)
    } else {
        None
    };

    // Step 9 is the validate + recap pass, driven by `create::finish`.
    Ok(MusicalWork::V1(MusicalWorkV1 {
        iswc,
        title,
        creation_year,
        instrumental,
        language,
        bpm,
        key,
        work_type,
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
    }
    let kind = prompts::select(
        "Work type",
        &[
            ("Original — standalone work", Kind::Original),
            ("Medley — works performed back-to-back", Kind::Medley),
            ("Mashup — works combined into one", Kind::Mashup),
            ("Adaptation — derived from one source", Kind::Adaptation),
        ],
        0,
    )?;
    Ok(match kind {
        Kind::Original => WorkType::Original,
        Kind::Medley | Kind::Mashup => {
            // Min 2 source ISWCs — fewer "is not a medley/mashup"
            // (`docs/validation.md` §4).
            let refs =
                prompts::collect_bounded("source ISWC", 2, WORK_REFERENCES_MAX as usize, |_| {
                    prompts::identifier::<11>("Source ISWC", validate_iswc_format, "T0345246801")
                })?;
            let refs = WorkReferences::try_from(refs)
                .map_err(|_| anyhow::anyhow!("more than {WORK_REFERENCES_MAX} source works"))?;
            if matches!(kind, Kind::Medley) {
                WorkType::Medley(refs)
            } else {
                WorkType::Mashup(refs)
            }
        }
        Kind::Adaptation => WorkType::Adaptation(prompts::identifier::<11>(
            "Source ISWC",
            validate_iswc_format,
            "T0345246801",
        )?),
    })
}

fn build_creators() -> Result<Creators> {
    let creators = prompts::collect_bounded(
        "creator",
        1,
        CREATORS_MAX as usize,
        |_| -> Result<Creator> {
            let role = prompts::select(
                "Role",
                &[
                    ("Author", CreatorRole::Author),
                    ("Composer", CreatorRole::Composer),
                    ("Arranger", CreatorRole::Arranger),
                    ("Adapter", CreatorRole::Adapter),
                    ("Publisher", CreatorRole::Publisher),
                ],
                1,
            )?;
            let id: CreatorId = shared::party_id("Creator identifier")?;
            Ok(Creator { role, id })
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
    // Optional, but a present value of 0 is nonsensical — the on-chain rule
    // requires `>= 1`.
    let number_of_voices = prompts::optional_int_in_range::<u16>("Number of voices", 1, u16::MAX)?;
    Ok(Some(ClassicalInfo {
        opus,
        catalog_number,
        number_of_voices,
    }))
}
