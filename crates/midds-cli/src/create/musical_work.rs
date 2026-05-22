//! Interactive builder for `MusicalWork::V1`.
//!
//! Each `ui::step` block maps one-to-one onto a `MusicalWorkV1` field group;
//! collection caps and numeric ranges come straight from `midds_types` /
//! `docs/validation.md` so the prompts and the on-chain `validate_format`
//! never disagree.

use anyhow::Result;
use midds_traits::{Iswc, validate_iswc_format};
use midds_types::shared::{BPM_MAX, BPM_MIN, YEAR_MAX, YEAR_MIN};
use midds_types::{
    CATALOG_NUMBER_MAX_LEN, CREATORS_MAX, ClassicalInfo, Creator, CreatorRole, CreatorRoles,
    Creators, MusicalWork, MusicalWorkV1, OPUS_MAX_LEN, TITLE_MAX_LEN, WORK_REFERENCES_MAX,
    WorkReferences, WorkType,
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
    let creation_year = prompts::optional_int_in_range::<u16>("Creation year", YEAR_MIN, YEAR_MAX)?;
    let instrumental = prompts::confirm("Instrumental (no lyrics)?", false)?;

    ui::step(3, STEPS, "Language");
    let language = prompts::optional("a language", || shared::language("Language"))?;

    ui::step(4, STEPS, "Tempo & key");
    let bpm = prompts::optional_int_in_range::<u16>("BPM", BPM_MIN, BPM_MAX)?;
    let key = prompts::optional("a musical key", || shared::musical_key("Musical key"))?;

    ui::step(5, STEPS, "Work type");
    let work_type = build_work_type()?;

    ui::step(6, STEPS, "Creators");
    let creators = build_creators()?;

    ui::step(7, STEPS, "Classical metadata");
    let classical_info = build_classical_info()?;

    ui::step(8, STEPS, "Off-chain extension");
    let offchain_extension = shared::offchain_extension()?;

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
