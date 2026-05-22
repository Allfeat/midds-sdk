//! Cross-type domain composites for the `create` wizard.
//!
//! These mirror the shapes in `midds_types::shared` (and the two ISO
//! tag-byte enums) that more than one builder needs: party identifiers,
//! work/recording references, a diatonic key, a country / language code, a
//! release date, and the off-chain extension hash. Kept apart from the
//! generic widgets in [`super::prompts`] — these know the MIDDS domain,
//! those don't.

use anyhow::{Context, Result};
use dialoguer::Input;
use midds_traits::{
    OffchainHash, validate_ipi_format, validate_isni_format, validate_isrc_format,
    validate_iswc_format, validate_offchain_hash,
};
use midds_types::{
    Country, Language, Mode, MusicalKey, PartyId, PitchClass, RecordingRef, ReleaseDate, WorkRef,
};

use crate::create::prompts;
use crate::ui;

/// A party identifier — IPI, ISNI, or both. Backs `artist`, every `creator`,
/// `performers`, `contributors`. The `Both` choice gets two consecutive
/// identifier prompts and yields `PartyId::Both { ipi, isni }`; the single-id
/// choices stay one-prompt wide for the fast path.
pub fn party_id(label: &str) -> Result<PartyId> {
    match prompts::select(
        label,
        &[
            ("IPI — 9–11 digits", 0u8),
            ("ISNI — 16 chars", 1u8),
            ("Both — IPI + ISNI", 2u8),
        ],
        0,
    )? {
        0 => Ok(PartyId::Ipi(prompts::identifier::<11>(
            "IPI",
            validate_ipi_format,
            "00052210040",
        )?)),
        1 => Ok(PartyId::Isni(prompts::identifier::<16>(
            "ISNI",
            validate_isni_format,
            "0000000121032683",
        )?)),
        _ => Ok(PartyId::Both {
            ipi: prompts::identifier::<11>("IPI", validate_ipi_format, "00052210040")?,
            isni: prompts::identifier::<16>("ISNI", validate_isni_format, "0000000121032683")?,
        }),
    }
}

/// A reference to a musical work: cheapest on-chain `MiddsId`, or an external
/// ISWC for a work not (yet) registered.
pub fn work_ref(label: &str) -> Result<WorkRef> {
    match prompts::select(
        label,
        &[("External ISWC", 0u8), ("On-chain MusicalWork id", 1u8)],
        0,
    )? {
        0 => Ok(WorkRef::Iswc(prompts::identifier::<11>(
            "ISWC",
            validate_iswc_format,
            "T0345246801",
        )?)),
        _ => Ok(WorkRef::Midds(prompts::midds_id("MusicalWork MIDDS id")?)),
    }
}

/// A reference to a recording: on-chain `MiddsId`, or an external ISRC.
pub fn recording_ref(label: &str) -> Result<RecordingRef> {
    match prompts::select(
        label,
        &[("External ISRC", 0u8), ("On-chain Recording id", 1u8)],
        0,
    )? {
        0 => Ok(RecordingRef::Isrc(prompts::identifier::<12>(
            "ISRC",
            validate_isrc_format,
            "USRC17607839",
        )?)),
        _ => Ok(RecordingRef::Midds(prompts::midds_id(
            "Recording MIDDS id",
        )?)),
    }
}

/// A diatonic key (pitch class + major/minor).
pub fn musical_key(label: &str) -> Result<MusicalKey> {
    ui::hint(label);
    let pitch = prompts::select(
        "Pitch class",
        &[
            ("C", PitchClass::C),
            ("C♯", PitchClass::CSharp),
            ("D♭", PitchClass::DFlat),
            ("D", PitchClass::D),
            ("D♯", PitchClass::DSharp),
            ("E♭", PitchClass::EFlat),
            ("E", PitchClass::E),
            ("F", PitchClass::F),
            ("F♯", PitchClass::FSharp),
            ("G♭", PitchClass::GFlat),
            ("G", PitchClass::G),
            ("G♯", PitchClass::GSharp),
            ("A♭", PitchClass::AFlat),
            ("A", PitchClass::A),
            ("A♯", PitchClass::ASharp),
            ("B♭", PitchClass::BFlat),
            ("B", PitchClass::B),
        ],
        0,
    )?;
    let mode = prompts::select("Mode", &[("Major", Mode::Major), ("Minor", Mode::Minor)], 0)?;
    Ok(MusicalKey { pitch, mode })
}

/// An ISO 3166-1 alpha-2 country code, resolved case-insensitively to the
/// closed `Country` enum (the field is a single SCALE tag byte on-chain).
pub fn country(label: &str) -> Result<Country> {
    let raw = Input::<String>::with_theme(&ui::theme())
        .with_prompt(format!(
            "{label} (ISO 3166-1 alpha-2 — e.g. US, FR, GB, JP)"
        ))
        .validate_with(|s: &String| -> Result<(), String> {
            Country::from_code_ignore_ascii_case(s.trim().as_bytes())
                .map(|_| ())
                .ok_or_else(|| "unknown ISO 3166-1 alpha-2 code".to_string())
        })
        .interact_text()
        .context("read country")?;
    Ok(Country::from_code_ignore_ascii_case(raw.trim().as_bytes())
        .expect("validated by the prompt loop"))
}

/// An ISO 639-1 alpha-2 language code, resolved case-insensitively to the
/// closed `Language` enum.
pub fn language(label: &str) -> Result<Language> {
    let raw = Input::<String>::with_theme(&ui::theme())
        .with_prompt(format!("{label} (ISO 639-1 alpha-2 — e.g. en, fr, es, ja)"))
        .validate_with(|s: &String| -> Result<(), String> {
            Language::from_code_ignore_ascii_case(s.trim().as_bytes())
                .map(|_| ())
                .ok_or_else(|| "unknown ISO 639-1 alpha-2 code".to_string())
        })
        .interact_text()
        .context("read language")?;
    Ok(Language::from_code_ignore_ascii_case(raw.trim().as_bytes())
        .expect("validated by the prompt loop"))
}

/// The `offchain_extension: Option<OffchainHash>` field present on every V1
/// root MIDDS type. Prompted identically across `MusicalWork` / `Recording`
/// / `Release`, so each wizard call site stays a one-liner.
pub fn offchain_extension() -> Result<Option<OffchainHash>> {
    prompts::optional("an off-chain extension hash", || {
        prompts::identifier::<64>(
            "Off-chain hash",
            validate_offchain_hash,
            "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi",
        )
    })
}

/// A strict calendar date. `year` is intentionally uncapped (announced /
/// future-dated releases stay representable — `docs/validation.md` §7);
/// `month`/`day` are structurally range-checked, matching the on-chain rule.
pub fn release_date(label: &str) -> Result<ReleaseDate> {
    ui::hint(label);
    let year = prompts::number::<u16>("Year", None)?;
    let month = prompts::int_in_range::<u8>("Month", 1, 12, None)?;
    let day = prompts::int_in_range::<u8>("Day", 1, 31, None)?;
    Ok(ReleaseDate { year, month, day })
}
