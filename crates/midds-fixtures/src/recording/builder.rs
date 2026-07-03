//! Test-ergonomic builder for `Recording::V1`.
//!
//! Distinct from `midds_validate::RecordingBuilder`, which is the
//! parser-tolerant runtime variant — it accepts free-form `&str` inputs and
//! aggregates per-field errors. This builder takes already-bounded bytes and
//! panics on bound overflow because tests should be deterministic and loud
//! when they construct invalid payloads by mistake. Mirrors
//! [`crate::musical_work::MusicalWorkBuilder`].

use bounded_collections::BoundedVec;
use midds_traits::{Isni, Isrc};
use midds_types::{
    Genre, MusicalKey, PartyId, Performer, Place, Recording, RecordingV1, RecordingVersion, Title,
    WorkRef,
};

/// Fluent builder over already-canonical inputs.
#[derive(Debug, Clone)]
pub struct RecordingBuilder {
    isrc: Isrc,
    title: Title,
    title_aliases: Vec<Title>,
    artist: PartyId,
    featuring: Vec<PartyId>,
    work: WorkRef,
    genre: Option<Genre>,
    sub_genre: Option<Genre>,
    record_year: Option<u16>,
    version_type: Option<RecordingVersion>,
    performers: Vec<Performer>,
    producers: Vec<Isni>,
    duration: Option<u32>,
    bpm: Option<u16>,
    key: Option<MusicalKey>,
    places: Option<(Option<Place>, Option<Place>, Option<Place>)>,
    contributors: Vec<PartyId>,
}

impl RecordingBuilder {
    /// Empty builder with valid defaults: a placeholder ISRC, a non-empty
    /// title `"Untitled Recording"`, an IPI artist with a checksum-correct
    /// synthetic code, and the work referenced by a placeholder ISWC.
    ///
    /// `build()` on a default builder produces a payload that
    /// `Midds::validate_format` accepts.
    #[must_use]
    pub fn new() -> Self {
        let default_isrc = crate::identifiers::isrc_for_index(0);
        let default_artist = PartyId::Ipi(crate::identifiers::ipi_from_stem(123_456_789, 9));
        let default_work = WorkRef::Iswc(crate::identifiers::iswc_for_index(0));
        Self {
            isrc: default_isrc,
            title: BoundedVec::try_from(b"Untitled Recording".to_vec()).expect("18 bytes < 256"),
            title_aliases: Vec::new(),
            artist: default_artist,
            featuring: Vec::new(),
            work: default_work,
            genre: None,
            sub_genre: None,
            record_year: None,
            version_type: None,
            performers: Vec::new(),
            producers: Vec::new(),
            duration: None,
            bpm: None,
            key: None,
            places: None,
            contributors: Vec::new(),
        }
    }

    /// Override the canonical identifier.
    #[must_use]
    pub fn isrc(mut self, isrc: Isrc) -> Self {
        self.isrc = isrc;
        self
    }

    /// Override the title. Panics if `bytes.len() > TITLE_MAX_LEN`.
    #[must_use]
    pub fn title(mut self, bytes: &[u8]) -> Self {
        self.title = BoundedVec::try_from(bytes.to_vec()).expect("title within bound");
        self
    }

    /// Append an alternative title. Panics on per-alias or list-length overflow.
    #[must_use]
    pub fn add_title_alias(mut self, bytes: &[u8]) -> Self {
        self.title_aliases
            .push(BoundedVec::try_from(bytes.to_vec()).expect("alias within bound"));
        self
    }

    #[must_use]
    pub fn artist(mut self, artist: PartyId) -> Self {
        self.artist = artist;
        self
    }

    /// Replace the featured-artists list. Panics at build if it exceeds
    /// `FEATURING_MAX`.
    #[must_use]
    pub fn featuring_unchecked(mut self, featuring: Vec<PartyId>) -> Self {
        self.featuring = featuring;
        self
    }

    #[must_use]
    pub fn work(mut self, work: WorkRef) -> Self {
        self.work = work;
        self
    }

    /// Set (or clear) the primary genre.
    #[must_use]
    pub fn genre(mut self, genre: Option<Genre>) -> Self {
        self.genre = genre;
        self
    }

    /// Set (or clear) the optional secondary genre.
    #[must_use]
    pub fn sub_genre(mut self, sub_genre: Option<Genre>) -> Self {
        self.sub_genre = sub_genre;
        self
    }

    #[must_use]
    pub fn record_year(mut self, year: u16) -> Self {
        self.record_year = Some(year);
        self
    }

    #[must_use]
    pub fn version_type(mut self, version: RecordingVersion) -> Self {
        self.version_type = Some(version);
        self
    }

    /// Replace the performers list. Panics if `performers.len() > PERFORMERS_MAX`.
    /// Each [`Performer`] carries its own (already-bounded) instrument list.
    #[must_use]
    pub fn performers_unchecked(mut self, performers: Vec<Performer>) -> Self {
        self.performers = performers;
        self
    }

    /// Replace the producers list. Panics if `producers.len() > PRODUCERS_MAX`.
    #[must_use]
    pub fn producers_unchecked(mut self, producers: Vec<Isni>) -> Self {
        self.producers = producers;
        self
    }

    #[must_use]
    pub fn duration(mut self, seconds: u32) -> Self {
        self.duration = Some(seconds);
        self
    }

    #[must_use]
    pub fn bpm(mut self, bpm: u16) -> Self {
        self.bpm = Some(bpm);
        self
    }

    #[must_use]
    pub fn key(mut self, key: MusicalKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Attach production places. Each present field must respect the
    /// `PLACE_MAX_LEN` bound.
    #[must_use]
    pub fn places(
        mut self,
        recording: Option<&[u8]>,
        mixing: Option<&[u8]>,
        mastering: Option<&[u8]>,
    ) -> Self {
        let to_place = |raw: &[u8]| Place::try_from(raw.to_vec()).expect("place within bound");
        self.places = Some((
            recording.map(to_place),
            mixing.map(to_place),
            mastering.map(to_place),
        ));
        self
    }

    /// Replace the contributors list. Panics if `contributors.len() > CONTRIBUTORS_MAX`.
    #[must_use]
    pub fn contributors_unchecked(mut self, contributors: Vec<PartyId>) -> Self {
        self.contributors = contributors;
        self
    }

    /// Finalise into a versioned `Recording::V1`. Does **not** call
    /// `Midds::validate_format` — callers wanting a guaranteed-valid payload
    /// should invoke it themselves on the returned value.
    #[must_use]
    pub fn build(self) -> Recording {
        let places =
            self.places.map(
                |(recording, mixing, mastering)| midds_types::ProductionPlaces {
                    recording,
                    mixing,
                    mastering,
                },
            );
        Recording::V1(RecordingV1 {
            isrc: self.isrc,
            title: self.title,
            title_aliases: BoundedVec::try_from(self.title_aliases)
                .expect("title aliases within bound"),
            artist: self.artist,
            featuring: BoundedVec::try_from(self.featuring).expect("featuring within bound"),
            work: self.work,
            genre: self.genre,
            sub_genre: self.sub_genre,
            record_year: self.record_year,
            version_type: self.version_type,
            performers: BoundedVec::try_from(self.performers).expect("performers within bound"),
            producers: BoundedVec::try_from(self.producers).expect("producers within bound"),
            duration: self.duration,
            bpm: self.bpm,
            key: self.key,
            places,
            contributors: BoundedVec::try_from(self.contributors)
                .expect("contributors within bound"),
        })
    }
}

impl Default for RecordingBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod tests {
    use super::*;
    use midds_traits::Midds as _;
    use midds_types::{Instrument, Mode, PerformerId, PitchClass};

    #[test]
    fn default_builder_validates() {
        let recording = RecordingBuilder::new().build();
        recording
            .validate_format()
            .expect("default payload validates on-chain");
    }

    #[test]
    fn override_each_field() {
        let recording = RecordingBuilder::new()
            .title(b"My Recording")
            .add_title_alias(b"Alt Title")
            .artist(PartyId::Isni(
                BoundedVec::try_from(b"0000000121032683".to_vec()).expect("16 bytes"),
            ))
            .featuring_unchecked(vec![PartyId::Ipi(
                BoundedVec::try_from(b"00000000171".to_vec()).expect("11 bytes"),
            )])
            .work(WorkRef::Midds(42))
            .genre(Some(Genre::Pop))
            .sub_genre(Some(Genre::Soul))
            .record_year(1999)
            .version_type(RecordingVersion::Live)
            .performers_unchecked(vec![Performer {
                id: PerformerId::Ipn(BoundedVec::try_from(b"12345678".to_vec()).expect("8 bytes")),
                instruments: BoundedVec::try_from(vec![
                    Instrument::ElectricGuitar,
                    Instrument::LeadVocals,
                ])
                .expect("2 instruments"),
            }])
            .duration(241)
            .bpm(128)
            .key(MusicalKey {
                pitch: PitchClass::A,
                mode: Mode::Minor,
            })
            .places(Some(b"Abbey Road"), None, Some(b"Sterling Sound"))
            .build();
        let Recording::V1(v) = recording;
        assert_eq!(v.title.as_slice(), b"My Recording");
        assert_eq!(v.title_aliases.len(), 1);
        assert_eq!(v.work, WorkRef::Midds(42));
        assert_eq!(v.genre, Some(Genre::Pop));
        assert_eq!(v.featuring.len(), 1);
        assert_eq!(v.sub_genre, Some(Genre::Soul));
        assert_eq!(v.performers.len(), 1);
        assert_eq!(v.performers[0].instruments.len(), 2);
        assert_eq!(v.record_year, Some(1999));
        assert_eq!(v.duration, Some(241));
        assert!(v.places.is_some());
    }

    #[test]
    #[should_panic(expected = "title within bound")]
    fn title_overflow_panics() {
        let too_long = vec![b'x'; 1024];
        let _ = RecordingBuilder::new().title(&too_long);
    }
}
