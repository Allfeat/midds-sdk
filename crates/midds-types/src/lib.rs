#![cfg_attr(not(feature = "std"), no_std)]

pub mod country;
pub mod language;
pub mod musical_work;
pub mod recording;
pub mod release;
pub mod shared;

pub use country::Country;
pub use language::Language;
pub use shared::{PartyId, RecordingRef, WorkRef};
// `Mode`, `MusicalKey`, `PitchClass`, `Title`, `TITLE_MAX_LEN` and
// `CreatorId` now live in `shared` but stay re-exported through
// `musical_work` (where `CreatorId` is an alias of `PartyId`), so existing
// `midds_types::*` paths are unchanged.
pub use musical_work::{
    CATALOG_NUMBER_MAX_LEN, CREATORS_MAX, CatalogNumber, ClassicalInfo, Creator, CreatorId,
    CreatorRole, Creators, Mode, MusicalKey, MusicalWork, MusicalWorkV1, OPUS_MAX_LEN, Opus,
    PitchClass, TITLE_MAX_LEN, Title, WORK_REFERENCES_MAX, WorkReferences, WorkType,
};
pub use recording::{
    CONTRIBUTORS_MAX, Contributors, GENRES_MAX, Genre, Genres, PERFORMERS_MAX, PLACE_MAX_LEN,
    PRODUCERS_MAX, Performers, Place, Producers, ProductionPlaces, Recording, RecordingV1,
    RecordingVersion, TITLE_ALIASES_MAX, TitleAliases,
};
// `release` re-exports its own collection bounds / aliases. The names that
// collide with `recording`'s (`PRODUCERS_MAX`, `Producers`,
// `TITLE_ALIASES_MAX`, `TitleAliases`) stay module-qualified
// (`release::Producers`) rather than re-exported flat, to keep the existing
// `midds_types::*` recording paths unambiguous.
pub use release::{
    COVER_CONTRIBUTORS_MAX, CoverContributorName, CoverContributors, DistributorName, Producer,
    Release, ReleaseDate, ReleaseFormat, ReleasePackaging, ReleaseStatus, ReleaseType, ReleaseV1,
    TRACKS_MAX, Tracks,
};
