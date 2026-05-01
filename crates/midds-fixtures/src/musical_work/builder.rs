//! Test-ergonomic builder for `MusicalWork::V1`.
//!
//! Distinct from `midds_validate::MusicalWorkBuilder`, which is the
//! parser-tolerant runtime variant — it accepts free-form `&str` inputs and
//! aggregates per-field errors. This builder takes already-bounded bytes and
//! panics on bound overflow because tests should be deterministic and loud
//! when they construct invalid payloads by mistake.

use frame_support::BoundedVec;
use midds_traits::{Iswc, OffchainHash};
use midds_types::{
    CatalogNumber, ClassicalInfo, Creator, Creators, Language, MusicalKey, MusicalWork,
    MusicalWorkV1, Opus, Title, WorkType,
};

/// Fluent builder over already-canonical inputs.
#[derive(Debug, Clone)]
pub struct MusicalWorkBuilder {
    iswc: Iswc,
    title: Title,
    creation_year: u16,
    instrumental: bool,
    language: Option<Language>,
    bpm: Option<u16>,
    key: Option<MusicalKey>,
    work_type: WorkType,
    creators: Creators,
    classical_info: Option<ClassicalInfo>,
    offchain_extension: Option<OffchainHash>,
}

impl MusicalWorkBuilder {
    /// Empty builder with valid defaults: a placeholder ISWC, a non-empty title
    /// `"Untitled"`, year 2024, non-instrumental, `WorkType::Original`, and a
    /// single composer with a checksum-correct synthetic IPI.
    ///
    /// `build()` on a default builder produces a payload that
    /// `Midds::validate_format` accepts.
    pub fn new() -> Self {
        let default_iswc = crate::identifiers::iswc_for_index(0);
        let default_creator = Creator {
            role: midds_types::CreatorRole::Composer,
            id: midds_types::CreatorId::Ipi(crate::identifiers::ipi_from_stem(123_456_789, 9)),
        };
        Self {
            iswc: default_iswc,
            title: BoundedVec::try_from(b"Untitled".to_vec()).expect("8 bytes < 256"),
            creation_year: 2024,
            instrumental: false,
            language: None,
            bpm: None,
            key: None,
            work_type: WorkType::Original,
            creators: BoundedVec::try_from(vec![default_creator]).expect("1 creator < 32"),
            classical_info: None,
            offchain_extension: None,
        }
    }

    /// Override the canonical identifier.
    pub fn iswc(mut self, iswc: Iswc) -> Self {
        self.iswc = iswc;
        self
    }

    /// Override the title. Panics if `bytes.len() > TITLE_MAX_LEN`.
    pub fn title(mut self, bytes: &[u8]) -> Self {
        self.title = BoundedVec::try_from(bytes.to_vec()).expect("title within bound");
        self
    }

    pub fn creation_year(mut self, year: u16) -> Self {
        self.creation_year = year;
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
        self.work_type = work_type;
        self
    }

    /// Replace the creators list with the supplied one. Panics if
    /// `creators.len() > CREATORS_MAX`.
    pub fn creators_unchecked(mut self, creators: Vec<Creator>) -> Self {
        self.creators = BoundedVec::try_from(creators).expect("creators within bound");
        self
    }

    /// Append a creator. Panics if the list is already at `CREATORS_MAX`.
    pub fn add_creator(mut self, creator: Creator) -> Self {
        self.creators
            .try_push(creator)
            .expect("creators within bound");
        self
    }

    /// Attach optional classical-music metadata. `opus` and `catalog_number`
    /// must respect their on-chain bounds.
    pub fn classical_info(
        mut self,
        opus: Option<&[u8]>,
        catalog_number: Option<&[u8]>,
        number_of_voices: Option<u16>,
    ) -> Self {
        let opus = opus.map(|raw| Opus::try_from(raw.to_vec()).expect("opus within bound"));
        let catalog_number = catalog_number
            .map(|raw| CatalogNumber::try_from(raw.to_vec()).expect("catalog within bound"));
        self.classical_info = Some(ClassicalInfo {
            opus,
            catalog_number,
            number_of_voices,
        });
        self
    }

    pub fn offchain_extension(mut self, bytes: &[u8]) -> Self {
        self.offchain_extension =
            Some(OffchainHash::try_from(bytes.to_vec()).expect("offchain hash within bound"));
        self
    }

    /// Finalise into a versioned `MusicalWork::V1`. Does **not** call
    /// `Midds::validate_format` — callers wanting a guaranteed-valid payload
    /// should invoke it themselves on the returned value.
    pub fn build(self) -> MusicalWork {
        MusicalWork::V1(MusicalWorkV1 {
            iswc: self.iswc,
            title: self.title,
            creation_year: self.creation_year,
            instrumental: self.instrumental,
            language: self.language,
            bpm: self.bpm,
            key: self.key,
            work_type: self.work_type,
            creators: self.creators,
            classical_info: self.classical_info,
            offchain_extension: self.offchain_extension,
        })
    }
}

impl Default for MusicalWorkBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use midds_traits::Midds as _;
    use midds_types::{CreatorId, CreatorRole};

    #[test]
    fn default_builder_validates() {
        let work = MusicalWorkBuilder::new().build();
        work.validate_format()
            .expect("default payload validates on-chain");
    }

    #[test]
    fn override_each_field() {
        let work = MusicalWorkBuilder::new()
            .title(b"My Work")
            .creation_year(1972)
            .instrumental(true)
            .bpm(120)
            .key(MusicalKey {
                pitch: midds_types::PitchClass::A,
                mode: midds_types::Mode::Minor,
            })
            .offchain_extension(b"bafkreigh2akiscaildc")
            .build();
        let MusicalWork::V1(v) = work;
        assert_eq!(v.title.as_slice(), b"My Work");
        assert_eq!(v.creation_year, 1972);
        assert!(v.instrumental);
        assert_eq!(v.bpm, Some(120));
        assert!(v.offchain_extension.is_some());
    }

    #[test]
    fn add_creator_appends() {
        let extra = Creator {
            role: CreatorRole::Author,
            id: CreatorId::Ipi(crate::identifiers::ipi_from_stem(987_654_321, 10)),
        };
        let work = MusicalWorkBuilder::new().add_creator(extra).build();
        let MusicalWork::V1(v) = work;
        assert_eq!(v.creators.len(), 2);
    }

    #[test]
    #[should_panic(expected = "title within bound")]
    fn title_overflow_panics() {
        let too_long = vec![b'x'; 1024];
        let _ = MusicalWorkBuilder::new().title(&too_long);
    }
}
