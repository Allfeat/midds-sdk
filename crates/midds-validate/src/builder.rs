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

use frame_support::BoundedVec;
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
