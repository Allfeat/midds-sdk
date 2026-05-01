#![cfg_attr(not(feature = "std"), no_std)]

pub mod language;
pub mod musical_work;

pub use language::Language;
pub use musical_work::{
    CATALOG_NUMBER_MAX_LEN, CREATORS_MAX, CatalogNumber, ClassicalInfo, Creator, CreatorId,
    CreatorRole, Creators, Mode, MusicalKey, MusicalWork, MusicalWorkV1, OPUS_MAX_LEN, Opus,
    PitchClass, TITLE_MAX_LEN, Title, WORK_REFERENCES_MAX, WorkReferences, WorkType,
};
