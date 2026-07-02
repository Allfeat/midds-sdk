// Shared curated `clippy::pedantic` policy — identical in every crate root;
// anything not listed here must stay pedantic-clean.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::if_not_else,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::option_option,
    clippy::return_self_not_must_use,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::wildcard_imports
)]
//! Canonical MIDDS payload definitions: [`MusicalWork`], [`Recording`] and
//! [`Release`].
//!
//! Each payload is a top-level **versioned enum** (adding a `V2` is additive
//! on the wire; migrations stay explicit `OnRuntimeUpgrade` impls), bounded
//! and `MaxEncodedLen` throughout — the same definitions serve as storage
//! format, JSON schema (`serde` feature, std-only) and builder target.
//! Cross-payload pieces live in [`shared`]; [`Country`] and [`Language`] are
//! closed ISO tag-byte enums. `no_std` by default.
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unsafe_code)]
#![warn(missing_docs)]

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
    CONTRIBUTORS_MAX, Contributors, FEATURING_MAX, Featuring, Genre, INSTRUMENTS_PER_PERFORMER_MAX,
    Instrument, PERFORMERS_MAX, PLACE_MAX_LEN, PRODUCERS_MAX, Performer, PerformerInstruments,
    Performers, Place, Producers, ProductionPlaces, Recording, RecordingV1, RecordingVersion,
    TITLE_ALIASES_MAX, TitleAliases,
};
pub use release::{
    COVER_CONTRIBUTORS_MAX, CoverContributorName, CoverContributors, DistributorName, Producer,
    Release, ReleaseDate, ReleaseFormat, ReleasePackaging, ReleaseStatus, ReleaseType, ReleaseV1,
    TRACKS_MAX, Track, Tracks,
};
pub use shared::{PartyId, PerformerId, RecordingRef, WorkRef};
