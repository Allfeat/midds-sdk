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
use midds_traits::{Ipi, Ipn, Isni, Isrc, Iswc, OffchainHash, Upc};
use midds_types::{
    Country, Language, Mode, MusicalKey, PartyId, PerformerId, PitchClass, RecordingRef,
    ReleaseDate, TITLE_MAX_LEN, Title, WorkRef,
};
use midds_validate::{parse_ipi, parse_ipn, parse_isni, parse_isrc, parse_iswc, parse_upc};

use crate::create::prompts;
use crate::ui;

/// Human-readable rule shown when [`parse_iswc`] rejects a typed ISWC. Returned
/// as `Result<_, String>` so [`prompts::identifier`] can surface the rule
/// verbatim instead of the raw `ParseError` debug name.
pub fn parse_iswc_msg(s: &str) -> Result<Iswc, String> {
    parse_iswc(s).map_err(|_| {
        "ISWC must be 'T' + 10 digits (e.g. T0345246801); dots and dashes are accepted".into()
    })
}

/// Human-readable rule shown when [`parse_isni`] rejects a typed ISNI.
pub fn parse_isni_msg(s: &str) -> Result<Isni, String> {
    parse_isni(s).map_err(|_| {
        "ISNI must be 16 characters: 15 digits + a final digit or X (spaces / dashes accepted)"
            .into()
    })
}

/// Human-readable rule shown when [`parse_ipi`] rejects a typed IPI. A short
/// input is left-padded with zeros to 11 digits, so a user can paste either
/// `00052210040` or `52210040` and get the same canonical IPI.
pub fn parse_ipi_msg(s: &str) -> Result<Ipi, String> {
    parse_ipi(s)
        .map_err(|_| "IPI must be 1–11 digits (auto-padded with leading zeros to 11)".into())
}

/// Human-readable rule shown when [`parse_ipn`] rejects a typed IPN. A short
/// input is left-padded with zeros to the canonical 8 digits — same UX as
/// IPI; the `I-` prefix is **not** accepted (IPN is a separate registry).
pub fn parse_ipn_msg(s: &str) -> Result<Ipn, String> {
    parse_ipn(s).map_err(|_| "IPN must be 1–8 digits (auto-padded with leading zeros to 8)".into())
}

/// Human-readable rule shown when [`parse_isrc`] rejects a typed ISRC. Kept
/// identical across the standalone `Recording.isrc` field and the tracklist
/// `RecordingRef::Isrc` prompt so a malformed ISRC reads the same everywhere.
pub fn parse_isrc_msg(s: &str) -> Result<Isrc, String> {
    parse_isrc(s).map_err(|_| {
        "ISRC must be 12 characters: 2 letters, then 3 alphanumerics, then 7 digits".into()
    })
}

/// Human-readable rule shown when [`parse_upc`] rejects a typed UPC/EAN.
pub fn parse_upc_msg(s: &str) -> Result<Upc, String> {
    parse_upc(s).map_err(|_| "UPC / EAN must be 12 or 13 digits (spaces / dashes accepted)".into())
}

/// Tiny non-empty check for the off-chain extension hash. There is no
/// `midds-validate` parser for this field — it is opaque on-chain — so the
/// prompt only enforces what `validate_offchain_hash` enforces (non-empty,
/// within the 64-byte bound).
pub fn parse_offchain_hash_msg(s: &str) -> Result<OffchainHash, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("off-chain hash must not be empty".into());
    }
    OffchainHash::try_from(trimmed.as_bytes().to_vec())
        .map_err(|_| "off-chain hash exceeds its 64-byte bound".into())
}

/// A party identifier — IPI, ISNI, or both. Backs `artist`, every `creator`,
/// `contributors`. The `Both` choice gets two consecutive identifier prompts
/// and yields `PartyId::Both { ipi, isni }`; the single-id choices stay
/// one-prompt wide for the fast path.
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
        0 => Ok(PartyId::Ipi(prompts::identifier(
            "IPI",
            parse_ipi_msg,
            "00052210040",
        )?)),
        1 => Ok(PartyId::Isni(prompts::identifier(
            "ISNI",
            parse_isni_msg,
            "0000000121032683",
        )?)),
        _ => Ok(PartyId::Both {
            ipi: prompts::identifier("IPI", parse_ipi_msg, "00052210040")?,
            isni: prompts::identifier("ISNI", parse_isni_msg, "0000000121032683")?,
        }),
    }
}

/// A performer identifier — IPN, IPI, or ISNI. Performer-specific (an IPN is
/// only issued to performers declared at a performer CMO); the IPI / ISNI
/// choices stay as a fallback when no IPN is available. Listed in the order
/// IPN → IPI → ISNI so the primary code is the default.
pub fn performer_id(label: &str) -> Result<PerformerId> {
    match prompts::select(
        label,
        &[
            ("IPN — 1–8 digits (auto-padded)", 0u8),
            ("IPI — 1–11 digits (auto-padded)", 1u8),
            ("ISNI — 16 chars", 2u8),
        ],
        0,
    )? {
        0 => Ok(PerformerId::Ipn(prompts::identifier(
            "IPN",
            parse_ipn_msg,
            "12345678",
        )?)),
        1 => Ok(PerformerId::Ipi(prompts::identifier(
            "IPI",
            parse_ipi_msg,
            "00052210040",
        )?)),
        _ => Ok(PerformerId::Isni(prompts::identifier(
            "ISNI",
            parse_isni_msg,
            "0000000121032683",
        )?)),
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
        0 => Ok(WorkRef::Iswc(prompts::identifier(
            "ISWC",
            parse_iswc_msg,
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
        0 => Ok(RecordingRef::Isrc(prompts::identifier_capped(
            "ISRC",
            parse_isrc_msg,
            "USRC17607839",
            12,
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
            ("B♯", PitchClass::BSharp),
            ("C♯", PitchClass::CSharp),
            ("D♭", PitchClass::DFlat),
            ("D", PitchClass::D),
            ("D♯", PitchClass::DSharp),
            ("E♭", PitchClass::EFlat),
            ("E", PitchClass::E),
            ("F♭", PitchClass::FFlat),
            ("F", PitchClass::F),
            ("E♯", PitchClass::ESharp),
            ("F♯", PitchClass::FSharp),
            ("G♭", PitchClass::GFlat),
            ("G", PitchClass::G),
            ("G♯", PitchClass::GSharp),
            ("A♭", PitchClass::AFlat),
            ("A", PitchClass::A),
            ("A♯", PitchClass::ASharp),
            ("B♭", PitchClass::BFlat),
            ("B", PitchClass::B),
            ("C♭", PitchClass::CFlat),
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
        prompts::identifier(
            "Off-chain hash",
            parse_offchain_hash_msg,
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

/// Alternative / localized titles collector, shared by the Recording and
/// Release forms (same prompts; each caller passes its own type-level cap).
pub fn title_aliases<B: TryFrom<Vec<Title>>>(max: usize) -> Result<B> {
    prompts::collect_bounded_into("title alias", 0, max, |_| {
        prompts::bounded_string::<TITLE_MAX_LEN>("Alias", true)
    })
}

/// Featured-artist ("feat.") collector, shared by the Recording and Release
/// forms. Featured artists carry the same [`PartyId`] as the main artist.
pub fn featuring<B: TryFrom<Vec<PartyId>>>(max: usize) -> Result<B> {
    prompts::collect_bounded_into("featured artist", 0, max, |_| {
        party_id("Featured artist identifier")
    })
}
