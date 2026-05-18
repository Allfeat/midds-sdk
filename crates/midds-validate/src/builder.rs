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
    CREATORS_MAX, Creator, CreatorId, CreatorRole, Language, MusicalKey, MusicalWork,
    MusicalWorkV1, TITLE_MAX_LEN, WorkType,
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
    bpm: Option<u16>,
    key: Option<MusicalKey>,
    work_type: Option<WorkType>,
    creators_raw: Vec<CreatorInput>,
    offchain_extension_raw: Option<String>,
}

#[derive(Debug, Clone)]
struct CreatorInput {
    role: CreatorRole,
    /// Raw IPI / ISNI as typed by the user.
    id: String,
    kind: CreatorIdKind,
}

#[derive(Debug, Clone, Copy)]
enum CreatorIdKind {
    Ipi,
    Isni,
}

impl MusicalWorkBuilder {
    /// Empty builder. `iswc`, `title`, `creation_year`, and at least one
    /// creator must be set before [`build`](Self::build).
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

    pub fn language(mut self, language: Language) -> Self {
        self.language = Some(language);
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
    /// format).
    pub fn add_creator(self, ipi: &str) -> Self {
        self.add_creator_with_role(CreatorRole::Composer, ipi)
    }

    /// Append a creator with an explicit role and IPI input.
    pub fn add_creator_with_role(mut self, role: CreatorRole, ipi: &str) -> Self {
        self.creators_raw.push(CreatorInput {
            role,
            id: ipi.to_string(),
            kind: CreatorIdKind::Ipi,
        });
        self
    }

    /// Append a creator identified by an ISNI.
    pub fn add_creator_isni(mut self, role: CreatorRole, isni: &str) -> Self {
        self.creators_raw.push(CreatorInput {
            role,
            id: isni.to_string(),
            kind: CreatorIdKind::Isni,
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
        // Mandatory presence checks short-circuit the per-field aggregation
        // because nothing useful can be said about absent inputs.
        let iswc_raw = self
            .iswc_raw
            .as_deref()
            .ok_or(BuildError::Missing("iswc"))?;
        let title_raw = self
            .title_raw
            .as_deref()
            .ok_or(BuildError::Missing("title"))?;
        let creation_year = self
            .creation_year
            .ok_or(BuildError::Missing("creation_year"))?;
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
            let parsed = match input.kind {
                CreatorIdKind::Ipi => parse_ipi(&input.id).map(CreatorId::Ipi),
                CreatorIdKind::Isni => parse_isni(&input.id).map(CreatorId::Isni),
            };
            match parsed {
                Ok(id) => creators.push(Creator {
                    role: input.role,
                    id,
                }),
                Err(e) => errors.push(FieldError {
                    field: creator_field_name(i),
                    message: format!("`{}`: {e}", input.id),
                }),
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

        // Every Option above carries `Some` here because `errors` is empty.
        let iswc = iswc.expect("no errors → iswc parsed");
        let title = title.expect("no errors → title parsed");
        let creators = creators_bv.expect("no errors → creators bounded");
        let offchain_extension = offchain.expect("no errors → offchain optional resolved");

        let v1 = MusicalWorkV1 {
            iswc,
            title,
            creation_year,
            instrumental: self.instrumental,
            language: self.language,
            bpm: self.bpm,
            key: self.key,
            work_type: self.work_type.unwrap_or(WorkType::Original),
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
        assert_eq!(v.creation_year, 1972);
        assert_eq!(v.creators.len(), 2);
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
}

// =============================================================================
// Recording
// =============================================================================

// `Iswc`, `OffchainHash`, `MusicalKey`, `TITLE_MAX_LEN`, `BoundedVec`,
// `parse_ipi`, `parse_isni`, `parse_iswc`, `BuildError`, `FieldError` are
// already imported at the top of this module for `MusicalWorkBuilder`.
use midds_traits::Isni;
use midds_types::{
    CONTRIBUTORS_MAX, GENRES_MAX, Genre, PERFORMERS_MAX, PLACE_MAX_LEN, PRODUCERS_MAX, PartyId,
    Place, ProductionPlaces, Recording, RecordingV1, RecordingVersion, TITLE_ALIASES_MAX, WorkRef,
};

use crate::parse::parse_isrc;

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
    performers_raw: Vec<PartyInput>,
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

    /// Append a performer identified by an IPI.
    pub fn add_performer_ipi(mut self, ipi: &str) -> Self {
        self.performers_raw.push(PartyInput {
            raw: ipi.to_string(),
            kind: PartyKind::Ipi,
        });
        self
    }

    /// Append a performer identified by an ISNI.
    pub fn add_performer_isni(mut self, isni: &str) -> Self {
        self.performers_raw.push(PartyInput {
            raw: isni.to_string(),
            kind: PartyKind::Isni,
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
        // Mandatory presence checks short-circuit the per-field aggregation
        // because nothing useful can be said about absent inputs (mirrors
        // `MusicalWorkBuilder::build`).
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
        let performers = parse_party_list(
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

        // Every Option above carries `Some` here because `errors` is empty.
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

/// Parse a bounded list of party identifiers (performers / contributors).
/// Aggregates per-entry failures and the list-length overflow.
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
