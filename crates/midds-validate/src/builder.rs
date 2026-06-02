//! Parser-tolerant builders that aggregate per-field errors.
//!
//! Distinct from the test-ergonomic builders in `midds-fixtures`, which
//! take already-bounded bytes and panic on overflow. The builders here
//! accept free-form `&str` inputs (anything `parse_*` accepts), normalise
//! them through the tolerant parsers, and surface all per-field failures
//! at once via [`BuildError::Fields`]. This is the right ergonomic
//! contract for CLI / wizard-style flows where a user typed several
//! fields and wants every problem flagged in one pass.
//!
//! Recording / Release will follow this template — see the body of
//! [`MusicalWorkBuilder::build`] for the aggregation pattern.

use std::sync::LazyLock;

use bounded_collections::BoundedVec;
use midds_traits::{Iswc, OffchainHash};
use midds_types::{
    CREATORS_MAX, Creator, CreatorRole, CreatorRoles, Language, MusicalKey, MusicalWork,
    MusicalWorkV1, PartyId, SAMPLES_MAX, TITLE_MAX_LEN, WorkRef, WorkType,
};

use crate::error::{BuildError, FieldError};
use crate::parse::{parse_ipi, parse_isni, parse_iswc};

/// Pre-computed `"creators[i]"` strings for `i in 0..CREATORS_MAX`. The leak
/// fires at most `CREATORS_MAX` times across the whole process — bounded
/// regardless of how many builders are run.
static CREATOR_FIELD_NAMES: LazyLock<[&'static str; CREATORS_MAX as usize]> = LazyLock::new(|| {
    core::array::from_fn(|i| Box::leak(format!("creators[{i}]").into_boxed_str()) as &'static str)
});

fn creator_field_name(index: usize) -> &'static str {
    CREATOR_FIELD_NAMES
        .get(index)
        .copied()
        .unwrap_or("creators[overflow]")
}

/// Parser-tolerant builder for `MusicalWork::V1`.
///
/// Inputs are stored verbatim and parsed/validated only on [`build`](Self::build),
/// where every failing field surfaces as a [`FieldError`] inside
/// `BuildError::Fields`. A user supplying three bad inputs gets all three
/// diagnostics, not just the first.
#[derive(Debug, Clone, Default)]
pub struct MusicalWorkBuilder {
    iswc_raw: Option<String>,
    title_raw: Option<String>,
    creation_year: Option<u16>,
    instrumental: bool,
    language: Option<Language>,
    explicit_lyrics: bool,
    bpm: Option<u16>,
    key: Option<MusicalKey>,
    work_type: Option<WorkType>,
    samples_raw: Vec<SampleInput>,
    creators_raw: Vec<CreatorInput>,
    offchain_extension_raw: Option<String>,
}

/// How a sampled work was referenced by the user: by on-chain MIDDS id (no
/// parsing) or by a free-form ISWC string (tolerant-parsed at build).
#[derive(Debug, Clone)]
enum SampleInput {
    Midds(u64),
    Iswc(String),
}

/// A creator the user is composing for a `MusicalWork`. Holds the roles
/// they intend to attribute plus the raw identifier inputs (IPI, ISNI, or
/// both) — each parsed/validated only at build time so that every failure
/// surfaces in one [`BuildError::Fields`] pass.
#[derive(Debug, Clone)]
struct CreatorInput {
    /// Roles attributed to the creator. Must be non-empty at build time;
    /// duplicates are silently deduplicated by the [`CreatorRoles`] set.
    roles: Vec<CreatorRole>,
    /// Raw IPI input, if any.
    ipi_raw: Option<String>,
    /// Raw ISNI input, if any.
    isni_raw: Option<String>,
}

impl MusicalWorkBuilder {
    /// Empty builder. `iswc`, `title`, and at least one creator must be set
    /// before [`build`](Self::build); `creation_year` is optional and left
    /// unset translates to `None` on the payload.
    pub fn new() -> Self {
        Self::default()
    }

    /// Free-form ISWC input. Anything [`parse_iswc`] accepts works:
    /// `T0345246802`, `T-034.524.680-2`, padded / lowercased / etc.
    pub fn iswc(mut self, s: &str) -> Self {
        self.iswc_raw = Some(s.to_string());
        self
    }

    /// Title. Trimmed of leading/trailing whitespace at build time.
    pub fn title(mut self, s: &str) -> Self {
        self.title_raw = Some(s.to_string());
        self
    }

    pub fn creation_year(mut self, year: u16) -> Self {
        self.creation_year = Some(year);
        self
    }

    pub fn instrumental(mut self, value: bool) -> Self {
        self.instrumental = value;
        self
    }

    pub fn explicit_lyrics(mut self, value: bool) -> Self {
        self.explicit_lyrics = value;
        self
    }

    pub fn language(mut self, language: Language) -> Self {
        self.language = Some(language);
        self
    }

    /// Reference a work sampled by this one, by its on-chain MIDDS id.
    pub fn add_sample_midds(mut self, id: u64) -> Self {
        self.samples_raw.push(SampleInput::Midds(id));
        self
    }

    /// Reference a work sampled by this one, by a free-form ISWC string
    /// (anything [`parse_iswc`] accepts).
    pub fn add_sample_iswc(mut self, iswc: &str) -> Self {
        self.samples_raw.push(SampleInput::Iswc(iswc.to_string()));
        self
    }

    pub fn bpm(mut self, bpm: u16) -> Self {
        self.bpm = Some(bpm);
        self
    }

    pub fn key(mut self, key: MusicalKey) -> Self {
        self.key = Some(key);
        self
    }

    pub fn work_type(mut self, work_type: WorkType) -> Self {
        self.work_type = Some(work_type);
        self
    }

    /// Append a creator with explicit IPI input (any [`parse_ipi`]-accepted
    /// format) and a single `Composer` role — convenience for the most
    /// common case.
    pub fn add_creator(self, ipi: &str) -> Self {
        self.add_creator_with_role(CreatorRole::Composer, ipi)
    }

    /// Append a creator with an explicit role and IPI input.
    pub fn add_creator_with_role(mut self, role: CreatorRole, ipi: &str) -> Self {
        self.creators_raw.push(CreatorInput {
            roles: vec![role],
            ipi_raw: Some(ipi.to_string()),
            isni_raw: None,
        });
        self
    }

    /// Append a creator identified by an ISNI with a single role.
    pub fn add_creator_isni(mut self, role: CreatorRole, isni: &str) -> Self {
        self.creators_raw.push(CreatorInput {
            roles: vec![role],
            ipi_raw: None,
            isni_raw: Some(isni.to_string()),
        });
        self
    }

    /// Append a creator carrying any combination of roles and either or both
    /// identifiers. At build time the roles list must be non-empty and at
    /// least one of `ipi` / `isni` must be supplied; otherwise the call
    /// surfaces as a `creators[i]` field error.
    pub fn add_creator_full(
        mut self,
        roles: Vec<CreatorRole>,
        ipi: Option<&str>,
        isni: Option<&str>,
    ) -> Self {
        self.creators_raw.push(CreatorInput {
            roles,
            ipi_raw: ipi.map(str::to_string),
            isni_raw: isni.map(str::to_string),
        });
        self
    }

    /// Off-chain extension hash (CIDv1 by client convention). Stored
    /// verbatim and bound-checked at build time.
    pub fn offchain_extension(mut self, bytes: &str) -> Self {
        self.offchain_extension_raw = Some(bytes.to_string());
        self
    }

    /// Validate and finalise into a `MusicalWork::V1`.
    ///
    /// Returns [`BuildError::Missing`] if a mandatory field is unset, or
    /// [`BuildError::Fields`] with one [`FieldError`] per failing input.
    pub fn build(self) -> Result<MusicalWork, BuildError> {
        let iswc_raw = self
            .iswc_raw
            .as_deref()
            .ok_or(BuildError::Missing("iswc"))?;
        let title_raw = self
            .title_raw
            .as_deref()
            .ok_or(BuildError::Missing("title"))?;
        if self.creators_raw.is_empty() {
            return Err(BuildError::Missing("creators"));
        }

        let mut errors: Vec<FieldError> = Vec::new();

        let iswc: Option<Iswc> = match parse_iswc(iswc_raw) {
            Ok(v) => Some(v),
            Err(e) => {
                errors.push(FieldError {
                    field: "iswc",
                    message: format!("`{iswc_raw}`: {e}"),
                });
                None
            }
        };

        let title_trimmed = title_raw.trim();
        let title = if title_trimmed.is_empty() {
            errors.push(FieldError {
                field: "title",
                message: "title is empty after trimming".into(),
            });
            None
        } else {
            match BoundedVec::try_from(title_trimmed.as_bytes().to_vec()) {
                Ok(t) => Some(t),
                Err(_) => {
                    errors.push(FieldError {
                        field: "title",
                        message: format!(
                            "title is {} bytes, exceeds {TITLE_MAX_LEN}-byte bound",
                            title_trimmed.len()
                        ),
                    });
                    None
                }
            }
        };

        let mut creators = Vec::with_capacity(self.creators_raw.len());
        for (i, input) in self.creators_raw.iter().enumerate() {
            let field = creator_field_name(i);
            // Parse each supplied identifier independently so the user gets
            // every failure surfaced in one pass, then assemble the PartyId
            // from whichever of the two parsed cleanly.
            let ipi = match input.ipi_raw.as_deref() {
                Some(raw) => match parse_ipi(raw) {
                    Ok(v) => Some(Some(v)),
                    Err(e) => {
                        errors.push(FieldError {
                            field,
                            message: format!("ipi `{raw}`: {e}"),
                        });
                        None
                    }
                },
                None => Some(None),
            };
            let isni = match input.isni_raw.as_deref() {
                Some(raw) => match parse_isni(raw) {
                    Ok(v) => Some(Some(v)),
                    Err(e) => {
                        errors.push(FieldError {
                            field,
                            message: format!("isni `{raw}`: {e}"),
                        });
                        None
                    }
                },
                None => Some(None),
            };
            let mut role_set = CreatorRoles::new();
            if input.roles.is_empty() {
                errors.push(FieldError {
                    field,
                    message: "no role supplied — a creator needs at least one role".into(),
                });
            } else {
                let mut overflowed = false;
                for r in &input.roles {
                    match role_set.try_insert(*r) {
                        Ok(_) => {}
                        Err(_) => overflowed = true,
                    }
                }
                if overflowed {
                    errors.push(FieldError {
                        field,
                        message: format!(
                            "more than {} distinct roles supplied",
                            midds_types::CREATOR_ROLES_MAX
                        ),
                    });
                }
            }
            let party = match (ipi, isni) {
                (Some(Some(ipi)), Some(Some(isni))) => Some(PartyId::Both { ipi, isni }),
                (Some(Some(ipi)), Some(None)) => Some(PartyId::Ipi(ipi)),
                (Some(None), Some(Some(isni))) => Some(PartyId::Isni(isni)),
                (Some(None), Some(None)) => {
                    errors.push(FieldError {
                        field,
                        message: "neither ipi nor isni supplied — a creator needs at least one"
                            .into(),
                    });
                    None
                }
                // One of the two failed to parse — the FieldError is already
                // recorded; nothing more to do for this creator.
                _ => None,
            };
            if let Some(party) = party
                && !role_set.is_empty()
            {
                creators.push(Creator {
                    roles: role_set,
                    party,
                });
            }
        }
        let creators_bv = if creators.len() > CREATORS_MAX as usize {
            errors.push(FieldError {
                field: "creators",
                message: format!(
                    "{} creators provided, exceeds {CREATORS_MAX} max",
                    creators.len()
                ),
            });
            None
        } else {
            BoundedVec::try_from(creators).ok()
        };

        // Sampled-work references: MIDDS ids pass through, ISWC strings are
        // tolerant-parsed; each failure surfaces as a `samples` field error.
        let mut samples: Vec<WorkRef> = Vec::with_capacity(self.samples_raw.len());
        for (i, s) in self.samples_raw.iter().enumerate() {
            match s {
                SampleInput::Midds(id) => samples.push(WorkRef::Midds(*id)),
                SampleInput::Iswc(raw) => match parse_iswc(raw) {
                    Ok(iswc) => samples.push(WorkRef::Iswc(iswc)),
                    Err(e) => errors.push(FieldError {
                        field: "samples",
                        message: format!("#{i} `{raw}`: {e}"),
                    }),
                },
            }
        }
        let samples_bv = if samples.len() > SAMPLES_MAX as usize {
            errors.push(FieldError {
                field: "samples",
                message: format!(
                    "{} samples provided, exceeds {SAMPLES_MAX} max",
                    samples.len()
                ),
            });
            None
        } else {
            BoundedVec::try_from(samples).ok()
        };

        let offchain = match self.offchain_extension_raw {
            Some(raw) => match OffchainHash::try_from(raw.into_bytes()) {
                Ok(h) if !h.is_empty() => Some(Some(h)),
                _ => {
                    errors.push(FieldError {
                        field: "offchain_extension",
                        message: "empty or larger than 64-byte bound".into(),
                    });
                    None
                }
            },
            None => Some(None),
        };

        if !errors.is_empty() {
            return Err(BuildError::Fields(errors));
        }

        let iswc = iswc.expect("no errors → iswc parsed");
        let title = title.expect("no errors → title parsed");
        let creators = creators_bv.expect("no errors → creators bounded");
        let samples = samples_bv.expect("no errors → samples bounded");
        let offchain_extension = offchain.expect("no errors → offchain optional resolved");

        let v1 = MusicalWorkV1 {
            iswc,
            title,
            creation_year: self.creation_year,
            instrumental: self.instrumental,
            language: self.language,
            explicit_lyrics: self.explicit_lyrics,
            bpm: self.bpm,
            key: self.key,
            work_type: self.work_type.unwrap_or(WorkType::Original),
            samples,
            creators,
            classical_info: None,
            offchain_extension,
        };
        Ok(MusicalWork::V1(v1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_types::CreatorRole;

    #[test]
    fn happy_path_parses_real_world_inputs() {
        let work = MusicalWorkBuilder::new()
            .iswc("T-034.524.680-2")
            .title("  My Work  ")
            .creation_year(1972)
            .instrumental(false)
            .add_creator_with_role(CreatorRole::Composer, "I-123456789")
            .add_creator_isni(CreatorRole::Author, "0000 0001 2103 2683")
            .build()
            .expect("happy path builds");
        let MusicalWork::V1(v) = work;
        assert_eq!(v.iswc.as_slice(), b"T0345246802");
        assert_eq!(v.title.as_slice(), b"My Work");
        assert_eq!(v.creation_year, Some(1972));
        assert_eq!(v.creators.len(), 2);
    }

    #[test]
    fn creation_year_is_optional() {
        let work = MusicalWorkBuilder::new()
            .iswc("T0345246802")
            .title("My Work")
            .add_creator("123456789")
            .build()
            .expect("creation_year omitted ⇒ Option=None ⇒ builds");
        let MusicalWork::V1(v) = work;
        assert_eq!(v.creation_year, None);
    }

    #[test]
    fn missing_iswc_fails_with_missing_field() {
        let res = MusicalWorkBuilder::new()
            .title("X")
            .creation_year(2024)
            .add_creator("123456789")
            .build();
        assert!(matches!(res, Err(BuildError::Missing("iswc"))));
    }

    #[test]
    fn missing_creators_fails_with_missing_field() {
        let res = MusicalWorkBuilder::new()
            .iswc("T0345246802")
            .title("X")
            .creation_year(2024)
            .build();
        assert!(matches!(res, Err(BuildError::Missing("creators"))));
    }

    /// The cardinal contract — three bad fields surface in one diagnostic
    /// list instead of stopping at the first failure.
    #[test]
    fn aggregates_all_field_errors() {
        let res = MusicalWorkBuilder::new()
            .iswc("not-an-iswc")
            .title("   ")
            .creation_year(2024)
            .add_creator("not-an-ipi")
            .build();
        let errors = match res {
            Err(BuildError::Fields(v)) => v,
            other => panic!("expected Fields(_), got {other:?}"),
        };
        let fields: Vec<&str> = errors.iter().map(|e| e.field).collect();
        assert!(fields.contains(&"iswc"));
        assert!(fields.contains(&"title"));
        assert!(fields.iter().any(|f| f.starts_with("creators")));
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn invalid_offchain_extension_surfaces_error() {
        let oversized: String = "h".repeat(128);
        let res = MusicalWorkBuilder::new()
            .iswc("T0345246802")
            .title("X")
            .creation_year(2024)
            .add_creator("123456789")
            .offchain_extension(&oversized)
            .build();
        let errors = match res {
            Err(BuildError::Fields(v)) => v,
            other => panic!("expected Fields(_), got {other:?}"),
        };
        assert_eq!(errors[0].field, "offchain_extension");
    }

    #[test]
    fn samples_and_explicit_lyrics_build_and_validate() {
        use midds_traits::Midds as _;
        let work = MusicalWorkBuilder::new()
            .iswc("T0345246802")
            .title("Sampling Work")
            .explicit_lyrics(true)
            .add_sample_iswc("T-098.765.432-1")
            .add_sample_midds(7)
            .add_creator("123456789")
            .build()
            .expect("builds");
        work.validate_format()
            .expect("builder output validates on-chain");
        let MusicalWork::V1(v) = work;
        assert!(v.explicit_lyrics);
        assert_eq!(v.samples.len(), 2);
        assert_eq!(
            v.samples[0],
            WorkRef::Iswc(parse_iswc("T0987654321").expect("canonical ISWC"))
        );
        assert_eq!(v.samples[1], WorkRef::Midds(7));
    }

    #[test]
    fn bad_sample_iswc_surfaces_error() {
        let res = MusicalWorkBuilder::new()
            .iswc("T0345246802")
            .title("X")
            .add_creator("123456789")
            .add_sample_iswc("not-an-iswc")
            .build();
        let errors = match res {
            Err(BuildError::Fields(v)) => v,
            other => panic!("expected Fields(_), got {other:?}"),
        };
        assert!(errors.iter().any(|e| e.field == "samples"));
    }
}

use midds_traits::Isni;
use midds_types::{
    CONTRIBUTORS_MAX, GENRES_MAX, Genre, PERFORMERS_MAX, PLACE_MAX_LEN, PRODUCERS_MAX, PerformerId,
    Place, ProductionPlaces, Recording, RecordingV1, RecordingVersion, TITLE_ALIASES_MAX,
};

use crate::parse::{parse_ipn, parse_isrc};

/// How to interpret a raw party-identifier string typed by the user.
#[derive(Debug, Clone, Copy)]
enum PartyKind {
    Ipi,
    Isni,
}

#[derive(Debug, Clone)]
struct PartyInput {
    raw: String,
    kind: PartyKind,
}

/// How to interpret a raw performer-identifier string. Distinct from
/// [`PartyKind`] because a performer carries the wider [`PerformerId`] enum
/// (IPN / IPI / ISNI) — non-performer parties cannot hold an IPN.
#[derive(Debug, Clone, Copy)]
enum PerformerKind {
    Ipn,
    Ipi,
    Isni,
}

#[derive(Debug, Clone)]
struct PerformerInput {
    raw: String,
    kind: PerformerKind,
}

/// How the recorded work was referenced by the user: by on-chain MIDDS id
/// (no parsing) or by a free-form ISWC string (tolerant-parsed at build).
#[derive(Debug, Clone)]
enum WorkInput {
    Midds(u64),
    Iswc(String),
}

/// Parser-tolerant builder for `Recording::V1`.
///
/// Same ergonomic contract as [`MusicalWorkBuilder`]: free-form `&str`
/// inputs are stored verbatim and only parsed/validated on
/// [`build`](Self::build), where every failing field surfaces as a
/// [`FieldError`] inside `BuildError::Fields` (a user supplying several bad
/// inputs gets every diagnostic, not just the first). Mandatory fields are
/// `isrc`, `title`, `artist`, and `work`.
#[derive(Debug, Clone, Default)]
pub struct RecordingBuilder {
    isrc_raw: Option<String>,
    title_raw: Option<String>,
    title_aliases_raw: Vec<String>,
    artist: Option<PartyInput>,
    work: Option<WorkInput>,
    genres: Vec<Genre>,
    record_year: Option<u16>,
    version_type: Option<RecordingVersion>,
    performers_raw: Vec<PerformerInput>,
    producers_raw: Vec<String>,
    duration: Option<u32>,
    bpm: Option<u16>,
    key: Option<MusicalKey>,
    places_raw: Option<(Option<String>, Option<String>, Option<String>)>,
    contributors_raw: Vec<PartyInput>,
    offchain_extension_raw: Option<String>,
}

impl RecordingBuilder {
    /// Empty builder. `isrc`, `title`, `artist`, and `work` must be set
    /// before [`build`](Self::build).
    pub fn new() -> Self {
        Self::default()
    }

    /// Free-form ISRC input. Anything [`parse_isrc`] accepts works:
    /// `USRC17607839`, `US-RC1-76-07839`, lowercased / padded / etc.
    pub fn isrc(mut self, s: &str) -> Self {
        self.isrc_raw = Some(s.to_string());
        self
    }

    /// Title. Trimmed of leading/trailing whitespace at build time.
    pub fn title(mut self, s: &str) -> Self {
        self.title_raw = Some(s.to_string());
        self
    }

    /// Append an alternative / localized title.
    pub fn add_title_alias(mut self, s: &str) -> Self {
        self.title_aliases_raw.push(s.to_string());
        self
    }

    /// Artist identified by an IPI (any [`parse_ipi`]-accepted format).
    pub fn artist_ipi(mut self, ipi: &str) -> Self {
        self.artist = Some(PartyInput {
            raw: ipi.to_string(),
            kind: PartyKind::Ipi,
        });
        self
    }

    /// Artist identified by an ISNI (any [`parse_isni`]-accepted format).
    pub fn artist_isni(mut self, isni: &str) -> Self {
        self.artist = Some(PartyInput {
            raw: isni.to_string(),
            kind: PartyKind::Isni,
        });
        self
    }

    /// Reference the recorded work by its on-chain MIDDS id.
    pub fn work_midds(mut self, id: u64) -> Self {
        self.work = Some(WorkInput::Midds(id));
        self
    }

    /// Reference the recorded work by a free-form ISWC string.
    pub fn work_iswc(mut self, iswc: &str) -> Self {
        self.work = Some(WorkInput::Iswc(iswc.to_string()));
        self
    }

    /// Replace the genres list (typed enum — no parsing).
    pub fn genres(mut self, genres: Vec<Genre>) -> Self {
        self.genres = genres;
        self
    }

    pub fn record_year(mut self, year: u16) -> Self {
        self.record_year = Some(year);
        self
    }

    pub fn version_type(mut self, version: RecordingVersion) -> Self {
        self.version_type = Some(version);
        self
    }

    /// Append a performer identified by an IPN (International Performer
    /// Number — issued by performer CMOs to declared performers).
    pub fn add_performer_ipn(mut self, ipn: &str) -> Self {
        self.performers_raw.push(PerformerInput {
            raw: ipn.to_string(),
            kind: PerformerKind::Ipn,
        });
        self
    }

    /// Append a performer identified by an IPI — the fallback for a performer
    /// not declared at a performer CMO but already registered on the
    /// publishing side.
    pub fn add_performer_ipi(mut self, ipi: &str) -> Self {
        self.performers_raw.push(PerformerInput {
            raw: ipi.to_string(),
            kind: PerformerKind::Ipi,
        });
        self
    }

    /// Append a performer identified by an ISNI.
    pub fn add_performer_isni(mut self, isni: &str) -> Self {
        self.performers_raw.push(PerformerInput {
            raw: isni.to_string(),
            kind: PerformerKind::Isni,
        });
        self
    }

    /// Append a producer (ISNI only — industry metadata identifies producers
    /// by ISNI, not IPI).
    pub fn add_producer(mut self, isni: &str) -> Self {
        self.producers_raw.push(isni.to_string());
        self
    }

    pub fn duration(mut self, seconds: u32) -> Self {
        self.duration = Some(seconds);
        self
    }

    pub fn bpm(mut self, bpm: u16) -> Self {
        self.bpm = Some(bpm);
        self
    }

    pub fn key(mut self, key: MusicalKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Attach production places. Empty inputs are rejected at build time —
    /// `validate_format` treats a present-but-empty place as a missing
    /// mandatory field.
    pub fn places(
        mut self,
        recording: Option<&str>,
        mixing: Option<&str>,
        mastering: Option<&str>,
    ) -> Self {
        self.places_raw = Some((
            recording.map(str::to_string),
            mixing.map(str::to_string),
            mastering.map(str::to_string),
        ));
        self
    }

    /// Append a contributor identified by an IPI.
    pub fn add_contributor_ipi(mut self, ipi: &str) -> Self {
        self.contributors_raw.push(PartyInput {
            raw: ipi.to_string(),
            kind: PartyKind::Ipi,
        });
        self
    }

    /// Append a contributor identified by an ISNI.
    pub fn add_contributor_isni(mut self, isni: &str) -> Self {
        self.contributors_raw.push(PartyInput {
            raw: isni.to_string(),
            kind: PartyKind::Isni,
        });
        self
    }

    /// Off-chain extension hash (CIDv1 by client convention). Stored
    /// verbatim and bound-checked at build time.
    pub fn offchain_extension(mut self, bytes: &str) -> Self {
        self.offchain_extension_raw = Some(bytes.to_string());
        self
    }

    /// Validate and finalise into a `Recording::V1`.
    ///
    /// Returns [`BuildError::Missing`] if a mandatory field is unset, or
    /// [`BuildError::Fields`] with one [`FieldError`] per failing input.
    pub fn build(self) -> Result<Recording, BuildError> {
        let isrc_raw = self
            .isrc_raw
            .as_deref()
            .ok_or(BuildError::Missing("isrc"))?;
        let title_raw = self
            .title_raw
            .as_deref()
            .ok_or(BuildError::Missing("title"))?;
        let artist_in = self.artist.as_ref().ok_or(BuildError::Missing("artist"))?;
        let work_in = self.work.as_ref().ok_or(BuildError::Missing("work"))?;

        let mut errors: Vec<FieldError> = Vec::new();

        let isrc = match parse_isrc(isrc_raw) {
            Ok(v) => Some(v),
            Err(e) => {
                errors.push(FieldError {
                    field: "isrc",
                    message: format!("`{isrc_raw}`: {e}"),
                });
                None
            }
        };

        let title_trimmed = title_raw.trim();
        let title = if title_trimmed.is_empty() {
            errors.push(FieldError {
                field: "title",
                message: "title is empty after trimming".into(),
            });
            None
        } else {
            match BoundedVec::try_from(title_trimmed.as_bytes().to_vec()) {
                Ok(t) => Some(t),
                Err(_) => {
                    errors.push(FieldError {
                        field: "title",
                        message: format!(
                            "title is {} bytes, exceeds {TITLE_MAX_LEN}-byte bound",
                            title_trimmed.len()
                        ),
                    });
                    None
                }
            }
        };

        let title_aliases = self.build_title_aliases(&mut errors);
        let artist = parse_party(artist_in, "artist", &mut errors);
        let work = self.resolve_work(work_in, &mut errors);
        let performers = parse_performer_list(
            &self.performers_raw,
            "performers",
            PERFORMERS_MAX,
            &mut errors,
        );
        let producers = self.build_producers(&mut errors);
        let contributors = parse_party_list(
            &self.contributors_raw,
            "contributors",
            CONTRIBUTORS_MAX,
            &mut errors,
        );
        let genres = if self.genres.len() > GENRES_MAX as usize {
            errors.push(FieldError {
                field: "genres",
                message: format!(
                    "{} genres provided, exceeds {GENRES_MAX} max",
                    self.genres.len()
                ),
            });
            None
        } else {
            BoundedVec::try_from(self.genres.clone()).ok()
        };
        let places = self.build_places(&mut errors);

        let offchain_extension = match &self.offchain_extension_raw {
            Some(raw) => match OffchainHash::try_from(raw.clone().into_bytes()) {
                Ok(h) if !h.is_empty() => Some(Some(h)),
                _ => {
                    errors.push(FieldError {
                        field: "offchain_extension",
                        message: "empty or larger than 64-byte bound".into(),
                    });
                    None
                }
            },
            None => Some(None),
        };

        if !errors.is_empty() {
            return Err(BuildError::Fields(errors));
        }

        let v1 = RecordingV1 {
            isrc: isrc.expect("no errors → isrc parsed"),
            title: title.expect("no errors → title parsed"),
            title_aliases: title_aliases.expect("no errors → aliases bounded"),
            artist: artist.expect("no errors → artist parsed"),
            work: work.expect("no errors → work resolved"),
            genres: genres.expect("no errors → genres bounded"),
            record_year: self.record_year,
            version_type: self.version_type,
            performers: performers.expect("no errors → performers bounded"),
            producers: producers.expect("no errors → producers bounded"),
            duration: self.duration,
            bpm: self.bpm,
            key: self.key,
            places: places.expect("no errors → places resolved"),
            contributors: contributors.expect("no errors → contributors bounded"),
            offchain_extension: offchain_extension.expect("no errors → offchain resolved"),
        };
        Ok(Recording::V1(v1))
    }

    fn build_title_aliases(
        &self,
        errors: &mut Vec<FieldError>,
    ) -> Option<midds_types::TitleAliases> {
        let mut aliases = Vec::with_capacity(self.title_aliases_raw.len());
        for (i, raw) in self.title_aliases_raw.iter().enumerate() {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                errors.push(FieldError {
                    field: "title_aliases",
                    message: format!("alias #{i} is empty after trimming"),
                });
                continue;
            }
            match BoundedVec::try_from(trimmed.as_bytes().to_vec()) {
                Ok(a) => aliases.push(a),
                Err(_) => errors.push(FieldError {
                    field: "title_aliases",
                    message: format!(
                        "alias #{i} is {} bytes, exceeds {TITLE_MAX_LEN}-byte bound",
                        trimmed.len()
                    ),
                }),
            }
        }
        if aliases.len() > TITLE_ALIASES_MAX as usize {
            errors.push(FieldError {
                field: "title_aliases",
                message: format!(
                    "{} aliases provided, exceeds {TITLE_ALIASES_MAX} max",
                    aliases.len()
                ),
            });
            return None;
        }
        BoundedVec::try_from(aliases).ok()
    }

    fn resolve_work(&self, work_in: &WorkInput, errors: &mut Vec<FieldError>) -> Option<WorkRef> {
        match work_in {
            WorkInput::Midds(id) => Some(WorkRef::Midds(*id)),
            WorkInput::Iswc(raw) => match crate::parse::parse_iswc(raw) {
                Ok(iswc) => Some(WorkRef::Iswc(iswc)),
                Err(e) => {
                    errors.push(FieldError {
                        field: "work",
                        message: format!("`{raw}`: {e}"),
                    });
                    None
                }
            },
        }
    }

    fn build_producers(&self, errors: &mut Vec<FieldError>) -> Option<midds_types::Producers> {
        let mut producers: Vec<Isni> = Vec::with_capacity(self.producers_raw.len());
        for (i, raw) in self.producers_raw.iter().enumerate() {
            match parse_isni(raw) {
                Ok(isni) => producers.push(isni),
                Err(e) => errors.push(FieldError {
                    field: "producers",
                    message: format!("#{i} `{raw}`: {e}"),
                }),
            }
        }
        if producers.len() > PRODUCERS_MAX as usize {
            errors.push(FieldError {
                field: "producers",
                message: format!(
                    "{} producers provided, exceeds {PRODUCERS_MAX} max",
                    producers.len()
                ),
            });
            return None;
        }
        BoundedVec::try_from(producers).ok()
    }

    fn build_places(&self, errors: &mut Vec<FieldError>) -> Option<Option<ProductionPlaces>> {
        let Some((rec, mix, mas)) = &self.places_raw else {
            return Some(None);
        };
        let mut ok = true;
        let mut to_place = |slot: &str, raw: &Option<String>| -> Option<Place> {
            let raw = raw.as_ref()?;
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                errors.push(FieldError {
                    field: "places",
                    message: format!("{slot} place is empty after trimming"),
                });
                ok = false;
                return None;
            }
            match Place::try_from(trimmed.as_bytes().to_vec()) {
                Ok(p) => Some(p),
                Err(_) => {
                    errors.push(FieldError {
                        field: "places",
                        message: format!(
                            "{slot} place is {} bytes, exceeds {PLACE_MAX_LEN}-byte bound",
                            trimmed.len()
                        ),
                    });
                    ok = false;
                    None
                }
            }
        };
        let recording = to_place("recording", rec);
        let mixing = to_place("mixing", mix);
        let mastering = to_place("mastering", mas);
        if !ok {
            return None;
        }
        Some(Some(ProductionPlaces {
            recording,
            mixing,
            mastering,
        }))
    }
}

/// Parse a single mandatory party identifier (the artist). On failure pushes
/// a `FieldError` under `field` and returns `None`.
fn parse_party(
    input: &PartyInput,
    field: &'static str,
    errors: &mut Vec<FieldError>,
) -> Option<PartyId> {
    let parsed = match input.kind {
        PartyKind::Ipi => parse_ipi(&input.raw).map(PartyId::Ipi),
        PartyKind::Isni => parse_isni(&input.raw).map(PartyId::Isni),
    };
    match parsed {
        Ok(id) => Some(id),
        Err(e) => {
            errors.push(FieldError {
                field,
                message: format!("`{}`: {e}", input.raw),
            });
            None
        }
    }
}

/// Parse a bounded list of party identifiers (contributors). Aggregates
/// per-entry failures and the list-length overflow.
fn parse_party_list<C: bounded_collections::Get<u32>>(
    inputs: &[PartyInput],
    field: &'static str,
    max: u32,
    errors: &mut Vec<FieldError>,
) -> Option<BoundedVec<PartyId, C>> {
    let mut parsed = Vec::with_capacity(inputs.len());
    for (i, input) in inputs.iter().enumerate() {
        let one = match input.kind {
            PartyKind::Ipi => parse_ipi(&input.raw).map(PartyId::Ipi),
            PartyKind::Isni => parse_isni(&input.raw).map(PartyId::Isni),
        };
        match one {
            Ok(id) => parsed.push(id),
            Err(e) => errors.push(FieldError {
                field,
                message: format!("#{i} `{}`: {e}", input.raw),
            }),
        }
    }
    if parsed.len() > max as usize {
        errors.push(FieldError {
            field,
            message: format!("{} provided, exceeds {max} max", parsed.len()),
        });
        return None;
    }
    BoundedVec::try_from(parsed).ok()
}

/// Parse a bounded list of performer identifiers. The performer variant of
/// [`parse_party_list`] — yields the wider [`PerformerId`] enum so the IPN
/// branch is reachable.
fn parse_performer_list<C: bounded_collections::Get<u32>>(
    inputs: &[PerformerInput],
    field: &'static str,
    max: u32,
    errors: &mut Vec<FieldError>,
) -> Option<BoundedVec<PerformerId, C>> {
    let mut parsed = Vec::with_capacity(inputs.len());
    for (i, input) in inputs.iter().enumerate() {
        let one = match input.kind {
            PerformerKind::Ipn => parse_ipn(&input.raw).map(PerformerId::Ipn),
            PerformerKind::Ipi => parse_ipi(&input.raw).map(PerformerId::Ipi),
            PerformerKind::Isni => parse_isni(&input.raw).map(PerformerId::Isni),
        };
        match one {
            Ok(id) => parsed.push(id),
            Err(e) => errors.push(FieldError {
                field,
                message: format!("#{i} `{}`: {e}", input.raw),
            }),
        }
    }
    if parsed.len() > max as usize {
        errors.push(FieldError {
            field,
            message: format!("{} provided, exceeds {max} max", parsed.len()),
        });
        return None;
    }
    BoundedVec::try_from(parsed).ok()
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod recording_tests {
    use super::*;

    #[test]
    fn happy_path_parses_real_world_inputs() {
        let recording = RecordingBuilder::new()
            .isrc("US-RC1-76-07839")
            .title("  My Recording  ")
            .add_title_alias(" Alt Title ")
            .artist_isni("0000 0001 2103 2683")
            .work_iswc("T-034.524.680-2")
            .genres(vec![Genre::Pop, Genre::Jazz])
            .record_year(1999)
            .version_type(RecordingVersion::Live)
            .add_performer_ipi("I-123456789")
            .add_producer("0000 0001 2103 2683")
            .duration(241)
            .build()
            .expect("happy path builds");
        let Recording::V1(v) = recording;
        assert_eq!(v.isrc.as_slice(), b"USRC17607839");
        assert_eq!(v.title.as_slice(), b"My Recording");
        assert_eq!(v.title_aliases.len(), 1);
        assert_eq!(v.title_aliases[0].as_slice(), b"Alt Title");
        assert_eq!(v.work, WorkRef::Iswc(parse_isrc_iswc()));
        assert_eq!(v.performers.len(), 1);
        assert_eq!(v.producers.len(), 1);
    }

    fn parse_isrc_iswc() -> midds_traits::Iswc {
        BoundedVec::try_from(b"T0345246802".to_vec()).unwrap()
    }

    #[test]
    fn work_by_midds_id_needs_no_parsing() {
        let recording = RecordingBuilder::new()
            .isrc("USRC17607839")
            .title("X")
            .artist_ipi("123456789")
            .work_midds(42)
            .build()
            .expect("midds-ref work builds");
        let Recording::V1(v) = recording;
        assert_eq!(v.work, WorkRef::Midds(42));
    }

    #[test]
    fn missing_isrc_fails_with_missing_field() {
        let res = RecordingBuilder::new()
            .title("X")
            .artist_ipi("123456789")
            .work_midds(1)
            .build();
        assert!(matches!(res, Err(BuildError::Missing("isrc"))));
    }

    #[test]
    fn missing_work_fails_with_missing_field() {
        let res = RecordingBuilder::new()
            .isrc("USRC17607839")
            .title("X")
            .artist_ipi("123456789")
            .build();
        assert!(matches!(res, Err(BuildError::Missing("work"))));
    }

    /// The cardinal contract — every bad field surfaces in one diagnostic
    /// list instead of stopping at the first failure.
    #[test]
    fn aggregates_all_field_errors() {
        let res = RecordingBuilder::new()
            .isrc("not-an-isrc")
            .title("   ")
            .artist_ipi("not-an-ipi")
            .work_iswc("not-an-iswc")
            .add_performer_isni("bad-isni")
            .build();
        let errors = match res {
            Err(BuildError::Fields(v)) => v,
            other => panic!("expected Fields(_), got {other:?}"),
        };
        let fields: Vec<&str> = errors.iter().map(|e| e.field).collect();
        assert!(fields.contains(&"isrc"));
        assert!(fields.contains(&"title"));
        assert!(fields.contains(&"artist"));
        assert!(fields.contains(&"work"));
        assert!(fields.contains(&"performers"));
        assert_eq!(errors.len(), 5);
    }

    #[test]
    fn empty_place_is_rejected() {
        let res = RecordingBuilder::new()
            .isrc("USRC17607839")
            .title("X")
            .artist_ipi("123456789")
            .work_midds(1)
            .places(Some("   "), None, None)
            .build();
        let errors = match res {
            Err(BuildError::Fields(v)) => v,
            other => panic!("expected Fields(_), got {other:?}"),
        };
        assert_eq!(errors[0].field, "places");
    }

    #[test]
    fn built_payload_passes_on_chain_validation() {
        use midds_traits::Midds as _;
        let recording = RecordingBuilder::new()
            .isrc("USRC17607839")
            .title("Valid Recording")
            .artist_isni("0000000121032683")
            .work_iswc("T0345246802")
            .places(Some("Abbey Road"), None, Some("Sterling Sound"))
            .build()
            .expect("builds");
        recording
            .validate_format()
            .expect("builder output validates on-chain");
    }
}

use midds_types::release::{
    CATALOG_NUMBER_MAX_LEN, COVER_CONTRIBUTOR_NAME_MAX_LEN, COVER_CONTRIBUTORS_MAX,
    DISTRIBUTOR_NAME_MAX_LEN, PRODUCERS_MAX as RELEASE_PRODUCERS_MAX,
    TITLE_ALIASES_MAX as RELEASE_TITLE_ALIASES_MAX, TRACKS_MAX,
};
use midds_types::{
    Country, Producer, RecordingRef, Release, ReleaseDate, ReleaseFormat, ReleasePackaging,
    ReleaseStatus, ReleaseType, ReleaseV1,
};

use crate::parse::parse_upc;

/// How a track was referenced by the user: by on-chain MIDDS id (no parsing)
/// or by a free-form ISRC string (tolerant-parsed at build).
#[derive(Debug, Clone)]
enum TrackInput {
    Midds(u64),
    Isrc(String),
}

/// A producer entry the user typed: an ISNI string plus its catalog number,
/// both parsed / bound-checked at build time.
#[derive(Debug, Clone)]
struct ProducerInput {
    isni_raw: String,
    catalog_raw: String,
}

/// Parser-tolerant builder for `Release::V1`.
///
/// Same ergonomic contract as [`RecordingBuilder`]: free-form `&str` inputs
/// are stored verbatim and only parsed/validated on [`build`](Self::build),
/// where every failing field surfaces as a [`FieldError`] inside
/// `BuildError::Fields`. Mandatory fields: `upc`, `title`, `artist`, at least
/// one track, `status`, `release_date`, `country`, `distributor_name`,
/// `release_type`, `format`, `packaging` (the typed enum / date / country
/// fields are set through dedicated methods and reported as
/// [`BuildError::Missing`] when absent).
#[derive(Debug, Clone, Default)]
pub struct ReleaseBuilder {
    upc_raw: Option<String>,
    title_raw: Option<String>,
    title_aliases_raw: Vec<String>,
    artist: Option<PartyInput>,
    tracks: Vec<TrackInput>,
    producers_raw: Vec<ProducerInput>,
    status: Option<ReleaseStatus>,
    release_date: Option<ReleaseDate>,
    country: Option<Country>,
    distributor_raw: Option<String>,
    release_type: Option<ReleaseType>,
    format: Option<ReleaseFormat>,
    packaging: Option<ReleasePackaging>,
    cover_contributors_raw: Vec<String>,
    offchain_extension_raw: Option<String>,
}

impl ReleaseBuilder {
    /// Empty builder. All mandatory fields must be set before
    /// [`build`](Self::build).
    pub fn new() -> Self {
        Self::default()
    }

    /// Free-form UPC / EAN input. Anything [`parse_upc`] accepts works:
    /// `036000291452`, `0-36000-29145-2`, space-grouped, etc.
    pub fn upc(mut self, s: &str) -> Self {
        self.upc_raw = Some(s.to_string());
        self
    }

    /// Title. Trimmed of leading/trailing whitespace at build time.
    pub fn title(mut self, s: &str) -> Self {
        self.title_raw = Some(s.to_string());
        self
    }

    /// Append an alternative / localized title.
    pub fn add_title_alias(mut self, s: &str) -> Self {
        self.title_aliases_raw.push(s.to_string());
        self
    }

    /// Artist identified by an IPI (any [`parse_ipi`]-accepted format).
    pub fn artist_ipi(mut self, ipi: &str) -> Self {
        self.artist = Some(PartyInput {
            raw: ipi.to_string(),
            kind: PartyKind::Ipi,
        });
        self
    }

    /// Artist identified by an ISNI (any [`parse_isni`]-accepted format).
    pub fn artist_isni(mut self, isni: &str) -> Self {
        self.artist = Some(PartyInput {
            raw: isni.to_string(),
            kind: PartyKind::Isni,
        });
        self
    }

    /// Append a track referenced by its on-chain MIDDS id.
    pub fn add_track_midds(mut self, id: u64) -> Self {
        self.tracks.push(TrackInput::Midds(id));
        self
    }

    /// Append a track referenced by a free-form ISRC string.
    pub fn add_track_isrc(mut self, isrc: &str) -> Self {
        self.tracks.push(TrackInput::Isrc(isrc.to_string()));
        self
    }

    /// Append a producer (ISNI + its catalog number).
    pub fn add_producer(mut self, isni: &str, catalog_number: &str) -> Self {
        self.producers_raw.push(ProducerInput {
            isni_raw: isni.to_string(),
            catalog_raw: catalog_number.to_string(),
        });
        self
    }

    pub fn status(mut self, status: ReleaseStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn release_date(mut self, year: u16, month: u8, day: u8) -> Self {
        self.release_date = Some(ReleaseDate { year, month, day });
        self
    }

    pub fn country(mut self, country: Country) -> Self {
        self.country = Some(country);
        self
    }

    /// Distributor name. Trimmed; must be non-empty within the bound.
    pub fn distributor_name(mut self, s: &str) -> Self {
        self.distributor_raw = Some(s.to_string());
        self
    }

    pub fn release_type(mut self, release_type: ReleaseType) -> Self {
        self.release_type = Some(release_type);
        self
    }

    pub fn format(mut self, format: ReleaseFormat) -> Self {
        self.format = Some(format);
        self
    }

    pub fn packaging(mut self, packaging: ReleasePackaging) -> Self {
        self.packaging = Some(packaging);
        self
    }

    /// Append a cover-artwork contributor name.
    pub fn add_cover_contributor(mut self, s: &str) -> Self {
        self.cover_contributors_raw.push(s.to_string());
        self
    }

    /// Off-chain extension hash (CIDv1 by client convention). Stored
    /// verbatim and bound-checked at build time.
    pub fn offchain_extension(mut self, bytes: &str) -> Self {
        self.offchain_extension_raw = Some(bytes.to_string());
        self
    }

    /// Validate and finalise into a `Release::V1`.
    ///
    /// Returns [`BuildError::Missing`] if a mandatory field is unset, or
    /// [`BuildError::Fields`] with one [`FieldError`] per failing input.
    pub fn build(self) -> Result<Release, BuildError> {
        let upc_raw = self.upc_raw.as_deref().ok_or(BuildError::Missing("upc"))?;
        let title_raw = self
            .title_raw
            .as_deref()
            .ok_or(BuildError::Missing("title"))?;
        let artist_in = self.artist.as_ref().ok_or(BuildError::Missing("artist"))?;
        let status = self.status.ok_or(BuildError::Missing("status"))?;
        let release_date = self
            .release_date
            .ok_or(BuildError::Missing("release_date"))?;
        let country = self.country.ok_or(BuildError::Missing("country"))?;
        let distributor_raw = self
            .distributor_raw
            .as_deref()
            .ok_or(BuildError::Missing("distributor_name"))?;
        let release_type = self
            .release_type
            .ok_or(BuildError::Missing("release_type"))?;
        let format = self.format.ok_or(BuildError::Missing("format"))?;
        let packaging = self.packaging.ok_or(BuildError::Missing("packaging"))?;
        if self.tracks.is_empty() {
            return Err(BuildError::Missing("tracks"));
        }

        let mut errors: Vec<FieldError> = Vec::new();

        let upc = match parse_upc(upc_raw) {
            Ok(v) => Some(v),
            Err(e) => {
                errors.push(FieldError {
                    field: "upc",
                    message: format!("`{upc_raw}`: {e}"),
                });
                None
            }
        };

        let title_trimmed = title_raw.trim();
        let title = if title_trimmed.is_empty() {
            errors.push(FieldError {
                field: "title",
                message: "title is empty after trimming".into(),
            });
            None
        } else {
            match BoundedVec::try_from(title_trimmed.as_bytes().to_vec()) {
                Ok(t) => Some(t),
                Err(_) => {
                    errors.push(FieldError {
                        field: "title",
                        message: format!(
                            "title is {} bytes, exceeds {TITLE_MAX_LEN}-byte bound",
                            title_trimmed.len()
                        ),
                    });
                    None
                }
            }
        };

        let title_aliases = self.build_title_aliases(&mut errors);
        let artist = parse_party(artist_in, "artist", &mut errors);
        let tracks = self.build_tracks(&mut errors);
        let producers = self.build_producers(&mut errors);

        let distributor_trimmed = distributor_raw.trim();
        let distributor_name = if distributor_trimmed.is_empty() {
            errors.push(FieldError {
                field: "distributor_name",
                message: "distributor name is empty after trimming".into(),
            });
            None
        } else {
            match BoundedVec::try_from(distributor_trimmed.as_bytes().to_vec()) {
                Ok(d) => Some(d),
                Err(_) => {
                    errors.push(FieldError {
                        field: "distributor_name",
                        message: format!(
                            "distributor name is {} bytes, exceeds \
                             {DISTRIBUTOR_NAME_MAX_LEN}-byte bound",
                            distributor_trimmed.len()
                        ),
                    });
                    None
                }
            }
        };

        let cover_contributors = self.build_cover_contributors(&mut errors);

        let offchain_extension = match &self.offchain_extension_raw {
            Some(raw) => match OffchainHash::try_from(raw.clone().into_bytes()) {
                Ok(h) if !h.is_empty() => Some(Some(h)),
                _ => {
                    errors.push(FieldError {
                        field: "offchain_extension",
                        message: "empty or larger than 64-byte bound".into(),
                    });
                    None
                }
            },
            None => Some(None),
        };

        if !errors.is_empty() {
            return Err(BuildError::Fields(errors));
        }

        let v1 = ReleaseV1 {
            upc: upc.expect("no errors → upc parsed"),
            title: title.expect("no errors → title parsed"),
            title_aliases: title_aliases.expect("no errors → aliases bounded"),
            artist: artist.expect("no errors → artist parsed"),
            tracks: tracks.expect("no errors → tracks resolved"),
            producers: producers.expect("no errors → producers bounded"),
            status,
            release_date,
            country,
            distributor_name: distributor_name.expect("no errors → distributor parsed"),
            release_type,
            format,
            packaging,
            cover_contributors: cover_contributors.expect("no errors → cover contributors bounded"),
            offchain_extension: offchain_extension.expect("no errors → offchain resolved"),
        };
        Ok(Release::V1(v1))
    }

    fn build_title_aliases(
        &self,
        errors: &mut Vec<FieldError>,
    ) -> Option<midds_types::release::TitleAliases> {
        let mut aliases = Vec::with_capacity(self.title_aliases_raw.len());
        for (i, raw) in self.title_aliases_raw.iter().enumerate() {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                errors.push(FieldError {
                    field: "title_aliases",
                    message: format!("alias #{i} is empty after trimming"),
                });
                continue;
            }
            match BoundedVec::try_from(trimmed.as_bytes().to_vec()) {
                Ok(a) => aliases.push(a),
                Err(_) => errors.push(FieldError {
                    field: "title_aliases",
                    message: format!(
                        "alias #{i} is {} bytes, exceeds {TITLE_MAX_LEN}-byte bound",
                        trimmed.len()
                    ),
                }),
            }
        }
        if aliases.len() > RELEASE_TITLE_ALIASES_MAX as usize {
            errors.push(FieldError {
                field: "title_aliases",
                message: format!(
                    "{} aliases provided, exceeds {RELEASE_TITLE_ALIASES_MAX} max",
                    aliases.len()
                ),
            });
            return None;
        }
        BoundedVec::try_from(aliases).ok()
    }

    fn build_tracks(&self, errors: &mut Vec<FieldError>) -> Option<midds_types::release::Tracks> {
        let mut tracks: Vec<RecordingRef> = Vec::with_capacity(self.tracks.len());
        for (i, t) in self.tracks.iter().enumerate() {
            match t {
                TrackInput::Midds(id) => tracks.push(RecordingRef::Midds(*id)),
                TrackInput::Isrc(raw) => match parse_isrc(raw) {
                    Ok(isrc) => tracks.push(RecordingRef::Isrc(isrc)),
                    Err(e) => errors.push(FieldError {
                        field: "tracks",
                        message: format!("#{i} `{raw}`: {e}"),
                    }),
                },
            }
        }
        if tracks.len() > TRACKS_MAX as usize {
            errors.push(FieldError {
                field: "tracks",
                message: format!("{} tracks provided, exceeds {TRACKS_MAX} max", tracks.len()),
            });
            return None;
        }
        BoundedVec::try_from(tracks).ok()
    }

    fn build_producers(
        &self,
        errors: &mut Vec<FieldError>,
    ) -> Option<midds_types::release::Producers> {
        let mut producers: Vec<Producer> = Vec::with_capacity(self.producers_raw.len());
        for (i, p) in self.producers_raw.iter().enumerate() {
            let isni = match parse_isni(&p.isni_raw) {
                Ok(v) => Some(v),
                Err(e) => {
                    errors.push(FieldError {
                        field: "producers",
                        message: format!("#{i} isni `{}`: {e}", p.isni_raw),
                    });
                    None
                }
            };
            let catalog_trimmed = p.catalog_raw.trim();
            let catalog_number = if catalog_trimmed.is_empty() {
                errors.push(FieldError {
                    field: "producers",
                    message: format!("#{i} catalog number is empty after trimming"),
                });
                None
            } else {
                match BoundedVec::try_from(catalog_trimmed.as_bytes().to_vec()) {
                    Ok(c) => Some(c),
                    Err(_) => {
                        errors.push(FieldError {
                            field: "producers",
                            message: format!(
                                "#{i} catalog number is {} bytes, exceeds \
                                 {CATALOG_NUMBER_MAX_LEN}-byte bound",
                                catalog_trimmed.len()
                            ),
                        });
                        None
                    }
                }
            };
            if let (Some(isni), Some(catalog_number)) = (isni, catalog_number) {
                producers.push(Producer {
                    isni,
                    catalog_number,
                });
            }
        }
        if producers.len() > RELEASE_PRODUCERS_MAX as usize {
            errors.push(FieldError {
                field: "producers",
                message: format!(
                    "{} producers provided, exceeds {RELEASE_PRODUCERS_MAX} max",
                    producers.len()
                ),
            });
            return None;
        }
        BoundedVec::try_from(producers).ok()
    }

    fn build_cover_contributors(
        &self,
        errors: &mut Vec<FieldError>,
    ) -> Option<midds_types::CoverContributors> {
        let mut names = Vec::with_capacity(self.cover_contributors_raw.len());
        for (i, raw) in self.cover_contributors_raw.iter().enumerate() {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                errors.push(FieldError {
                    field: "cover_contributors",
                    message: format!("#{i} is empty after trimming"),
                });
                continue;
            }
            match BoundedVec::try_from(trimmed.as_bytes().to_vec()) {
                Ok(n) => names.push(n),
                Err(_) => errors.push(FieldError {
                    field: "cover_contributors",
                    message: format!(
                        "#{i} is {} bytes, exceeds {COVER_CONTRIBUTOR_NAME_MAX_LEN}-byte bound",
                        trimmed.len()
                    ),
                }),
            }
        }
        if names.len() > COVER_CONTRIBUTORS_MAX as usize {
            errors.push(FieldError {
                field: "cover_contributors",
                message: format!(
                    "{} cover contributors provided, exceeds {COVER_CONTRIBUTORS_MAX} max",
                    names.len()
                ),
            });
            return None;
        }
        BoundedVec::try_from(names).ok()
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod release_tests {
    use super::*;
    use midds_traits::Midds as _;

    #[test]
    fn happy_path_builds_and_validates() {
        let release = ReleaseBuilder::new()
            .upc("0-36000-29145-2")
            .title("  Transformer  ")
            .add_title_alias("Transformeur")
            .artist_isni("0000 0001 2103 2683")
            .add_track_isrc("US-RC1-72-00312")
            .add_track_midds(7)
            .add_producer("000000012103268X", "RCA LSP-4807")
            .status(ReleaseStatus::Official)
            .release_date(1972, 11, 8)
            .country(Country::Us)
            .distributor_name("RCA Records")
            .release_type(ReleaseType::Album)
            .format(ReleaseFormat::Vinyl)
            .packaging(ReleasePackaging::Gatefold)
            .add_cover_contributor("Mick Rock")
            .offchain_extension("bafkreigh2akiscaildc")
            .build()
            .expect("builds");
        release
            .validate_format()
            .expect("builder output validates on-chain");
        let Release::V1(v) = release;
        assert_eq!(v.upc.as_slice(), b"036000291452");
        assert_eq!(v.title.as_slice(), b"Transformer");
        assert_eq!(v.tracks.len(), 2);
        assert_eq!(v.producers.len(), 1);
    }

    #[test]
    fn missing_mandatory_field_reported() {
        let res = ReleaseBuilder::new().title("X").build();
        assert!(matches!(res, Err(BuildError::Missing("upc"))));
    }

    #[test]
    fn missing_tracklist_reported() {
        let res = ReleaseBuilder::new()
            .upc("036000291452")
            .title("X")
            .artist_ipi("123456789")
            .status(ReleaseStatus::Official)
            .release_date(2024, 1, 1)
            .country(Country::Us)
            .distributor_name("D")
            .release_type(ReleaseType::Album)
            .format(ReleaseFormat::Cd)
            .packaging(ReleasePackaging::None)
            .build();
        assert!(matches!(res, Err(BuildError::Missing("tracks"))));
    }

    #[test]
    fn aggregates_multiple_field_errors() {
        let res = ReleaseBuilder::new()
            .upc("not-a-upc")
            .title("   ")
            .artist_ipi("not-an-ipi")
            .add_track_isrc("bad-isrc")
            .status(ReleaseStatus::Official)
            .release_date(2024, 1, 1)
            .country(Country::Us)
            .distributor_name("D")
            .release_type(ReleaseType::Album)
            .format(ReleaseFormat::Cd)
            .packaging(ReleasePackaging::None)
            .build();
        let errors = match res {
            Err(BuildError::Fields(v)) => v,
            other => panic!("expected Fields(_), got {other:?}"),
        };
        let fields: Vec<_> = errors.iter().map(|e| e.field).collect();
        assert!(fields.contains(&"upc"));
        assert!(fields.contains(&"title"));
        assert!(fields.contains(&"artist"));
        assert!(fields.contains(&"tracks"));
    }

    #[test]
    fn empty_producer_catalog_is_rejected() {
        let res = ReleaseBuilder::new()
            .upc("036000291452")
            .title("X")
            .artist_ipi("123456789")
            .add_track_midds(1)
            .add_producer("0000000121032683", "  ")
            .status(ReleaseStatus::Official)
            .release_date(2024, 1, 1)
            .country(Country::Us)
            .distributor_name("D")
            .release_type(ReleaseType::Album)
            .format(ReleaseFormat::Cd)
            .packaging(ReleasePackaging::None)
            .build();
        let errors = match res {
            Err(BuildError::Fields(v)) => v,
            other => panic!("expected Fields(_), got {other:?}"),
        };
        assert_eq!(errors[0].field, "producers");
    }
}
