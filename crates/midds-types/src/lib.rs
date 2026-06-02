#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod country;
pub mod language;
pub mod musical_work;
pub mod recording;
pub mod release;
pub mod shared;

pub use country::Country;
pub use language::Language;
pub use musical_work::{
    CATALOG_NUMBER_MAX_LEN, CREATOR_ROLES_MAX, CREATORS_MAX, CatalogNumber, ClassicalInfo, Creator,
    CreatorRole, CreatorRoles, Creators, Mode, MusicalKey, MusicalWork, MusicalWorkV1,
    OPUS_MAX_LEN, Opus, PitchClass, SAMPLES_MAX, SampleReferences, TITLE_MAX_LEN, Title,
    WORK_REFERENCES_MAX, WorkReferences, WorkType,
};
pub use recording::{
    CONTRIBUTORS_MAX, Contributors, FEATURING_MAX, Featuring, GENRES_MAX, Genre, Genres,
    INSTRUMENTS_PER_PERFORMER_MAX, Instrument, PERFORMERS_MAX, PLACE_MAX_LEN, PRODUCERS_MAX,
    Performer, PerformerInstruments, Performers, Place, Producers, ProductionPlaces, Recording,
    RecordingV1, RecordingVersion, TITLE_ALIASES_MAX, TitleAliases,
};
pub use release::{
    COVER_CONTRIBUTORS_MAX, CoverContributorName, CoverContributors, DistributorName, Producer,
    Release, ReleaseDate, ReleaseFormat, ReleasePackaging, ReleaseStatus, ReleaseType, ReleaseV1,
    TRACKS_MAX, Tracks,
};
pub use shared::{PartyId, PerformerId, RecordingRef, WorkRef};
