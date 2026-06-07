//! Test-ergonomic builder for `Release::V1`.
//!
//! Distinct from `midds_validate::ReleaseBuilder`, which is the
//! parser-tolerant runtime variant — it accepts free-form `&str` inputs and
//! aggregates per-field errors. This builder takes already-bounded bytes and
//! panics on bound overflow because tests should be deterministic and loud
//! when they construct invalid payloads by mistake. Mirrors
//! [`crate::recording::RecordingBuilder`].

use frame_support::BoundedVec;
use midds_traits::{OffchainHash, Upc};
use midds_types::release::{Featuring, Producers, TitleAliases, Tracks};
use midds_types::{
    Country, CoverContributorName, CoverContributors, DistributorName, PartyId, Producer,
    RecordingRef, Release, ReleaseDate, ReleaseFormat, ReleasePackaging, ReleaseStatus,
    ReleaseType, ReleaseV1, Title, Track,
};

/// Fluent builder over already-canonical inputs.
#[derive(Debug, Clone)]
pub struct ReleaseBuilder {
    upc: Upc,
    title: Title,
    title_aliases: Vec<Title>,
    artist: PartyId,
    featuring: Vec<PartyId>,
    tracks: Vec<RecordingRef>,
    producers: Vec<Producer>,
    status: ReleaseStatus,
    release_date: ReleaseDate,
    country: Country,
    distributor_name: DistributorName,
    release_type: ReleaseType,
    format: ReleaseFormat,
    packaging: ReleasePackaging,
    cover_contributors: Vec<CoverContributorName>,
    offchain_extension: Option<OffchainHash>,
}

impl ReleaseBuilder {
    /// Empty builder with valid defaults: a placeholder EAN-13, a non-empty
    /// title `"Untitled Release"`, an IPI artist with a checksum-correct
    /// synthetic code, a single on-chain track, and the mandatory enum /
    /// date / country fields populated.
    ///
    /// `build()` on a default builder produces a payload that
    /// `Midds::validate_format` accepts.
    pub fn new() -> Self {
        Self {
            upc: crate::identifiers::upc_for_index(0),
            title: BoundedVec::try_from(b"Untitled Release".to_vec()).expect("16 bytes < 256"),
            title_aliases: Vec::new(),
            artist: PartyId::Ipi(crate::identifiers::ipi_from_stem(123_456_789, 9)),
            featuring: Vec::new(),
            tracks: vec![RecordingRef::Midds(0)],
            producers: Vec::new(),
            status: ReleaseStatus::Official,
            release_date: ReleaseDate {
                year: 2024,
                month: 1,
                day: 1,
            },
            country: Country::Us,
            distributor_name: BoundedVec::try_from(b"Unknown Distributor".to_vec())
                .expect("19 bytes < 128"),
            release_type: ReleaseType::Album,
            format: ReleaseFormat::Cd,
            packaging: ReleasePackaging::None,
            cover_contributors: Vec::new(),
            offchain_extension: None,
        }
    }

    /// Override the canonical identifier.
    pub fn upc(mut self, upc: Upc) -> Self {
        self.upc = upc;
        self
    }

    /// Override the title. Panics if `bytes.len() > TITLE_MAX_LEN`.
    pub fn title(mut self, bytes: &[u8]) -> Self {
        self.title = BoundedVec::try_from(bytes.to_vec()).expect("title within bound");
        self
    }

    /// Append an alternative title. Panics on per-alias or list-length overflow.
    pub fn add_title_alias(mut self, bytes: &[u8]) -> Self {
        self.title_aliases
            .push(BoundedVec::try_from(bytes.to_vec()).expect("alias within bound"));
        self
    }

    pub fn artist(mut self, artist: PartyId) -> Self {
        self.artist = artist;
        self
    }

    /// Replace the featured-artist list. Panics at `build()` if
    /// `featuring.len() > FEATURING_MAX`.
    pub fn featuring_unchecked(mut self, featuring: Vec<PartyId>) -> Self {
        self.featuring = featuring;
        self
    }

    /// Replace the tracklist. Panics if `tracks.len() > TRACKS_MAX`.
    pub fn tracks_unchecked(mut self, tracks: Vec<RecordingRef>) -> Self {
        self.tracks = tracks;
        self
    }

    /// Append one track reference.
    pub fn add_track(mut self, track: RecordingRef) -> Self {
        self.tracks.push(track);
        self
    }

    /// Replace the producers list. Panics if `producers.len() > PRODUCERS_MAX`.
    pub fn producers_unchecked(mut self, producers: Vec<Producer>) -> Self {
        self.producers = producers;
        self
    }

    pub fn status(mut self, status: ReleaseStatus) -> Self {
        self.status = status;
        self
    }

    pub fn release_date(mut self, year: u16, month: u8, day: u8) -> Self {
        self.release_date = ReleaseDate { year, month, day };
        self
    }

    pub fn country(mut self, country: Country) -> Self {
        self.country = country;
        self
    }

    /// Override the distributor name. Panics if it exceeds the bound.
    pub fn distributor_name(mut self, bytes: &[u8]) -> Self {
        self.distributor_name =
            BoundedVec::try_from(bytes.to_vec()).expect("distributor name within bound");
        self
    }

    pub fn release_type(mut self, release_type: ReleaseType) -> Self {
        self.release_type = release_type;
        self
    }

    pub fn format(mut self, format: ReleaseFormat) -> Self {
        self.format = format;
        self
    }

    pub fn packaging(mut self, packaging: ReleasePackaging) -> Self {
        self.packaging = packaging;
        self
    }

    /// Append a cover-artwork contributor name. Panics on per-name or
    /// list-length overflow.
    pub fn add_cover_contributor(mut self, bytes: &[u8]) -> Self {
        self.cover_contributors
            .push(BoundedVec::try_from(bytes.to_vec()).expect("cover contributor within bound"));
        self
    }

    pub fn offchain_extension(mut self, bytes: &[u8]) -> Self {
        self.offchain_extension =
            Some(OffchainHash::try_from(bytes.to_vec()).expect("offchain hash within bound"));
        self
    }

    /// Finalise into a versioned `Release::V1`. Does **not** call
    /// `Midds::validate_format` — callers wanting a guaranteed-valid payload
    /// should invoke it themselves on the returned value.
    pub fn build(self) -> Release {
        Release::V1(ReleaseV1 {
            upc: self.upc,
            title: self.title,
            title_aliases: TitleAliases::try_from(self.title_aliases)
                .expect("title aliases within bound"),
            artist: self.artist,
            featuring: Featuring::try_from(self.featuring).expect("featuring within bound"),
            tracks: Tracks::try_from(
                self.tracks
                    .into_iter()
                    .enumerate()
                    .map(|(i, recording)| Track {
                        number: (i + 1) as u16,
                        recording,
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("tracks within bound"),
            producers: Producers::try_from(self.producers).expect("producers within bound"),
            status: self.status,
            release_date: self.release_date,
            country: self.country,
            distributor_name: self.distributor_name,
            release_type: self.release_type,
            format: self.format,
            packaging: self.packaging,
            cover_contributors: CoverContributors::try_from(self.cover_contributors)
                .expect("cover contributors within bound"),
            offchain_extension: self.offchain_extension,
        })
    }
}

impl Default for ReleaseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, reason = "tests legitimately unwrap")]
mod tests {
    use super::*;
    use midds_traits::Midds as _;
    use midds_types::release::CatalogNumber;

    fn isni(s: &[u8]) -> midds_traits::Isni {
        BoundedVec::try_from(s.to_vec()).expect("16-byte ISNI")
    }

    #[test]
    fn default_builder_validates() {
        let release = ReleaseBuilder::new().build();
        release
            .validate_format()
            .expect("default payload validates on-chain");
    }

    #[test]
    fn override_each_field() {
        let release = ReleaseBuilder::new()
            .title(b"Transformer")
            .add_title_alias(b"Transformeur")
            .artist(PartyId::Isni(isni(b"0000000121032683")))
            .tracks_unchecked(vec![
                RecordingRef::Isrc(
                    BoundedVec::try_from(b"USRC17200312".to_vec()).expect("12-byte ISRC"),
                ),
                RecordingRef::Midds(7),
            ])
            .producers_unchecked(vec![Producer {
                isni: isni(b"000000012103268X"),
                catalog_number: CatalogNumber::try_from(b"RCA LSP-4807".to_vec())
                    .expect("catalog number"),
            }])
            .status(ReleaseStatus::Promotional)
            .release_date(1972, 11, 8)
            .country(Country::Us)
            .distributor_name(b"RCA Records")
            .release_type(ReleaseType::Album)
            .format(ReleaseFormat::Vinyl)
            .packaging(ReleasePackaging::Gatefold)
            .add_cover_contributor(b"Mick Rock")
            .offchain_extension(b"bafkreigh2akiscaildc")
            .build();
        release.validate_format().expect("validates");
        let Release::V1(v) = release;
        assert_eq!(v.title.as_slice(), b"Transformer");
        assert_eq!(v.title_aliases.len(), 1);
        assert_eq!(v.tracks.len(), 2);
        assert_eq!(v.producers.len(), 1);
        assert_eq!(v.cover_contributors.len(), 1);
        assert!(v.offchain_extension.is_some());
    }

    #[test]
    #[should_panic(expected = "title within bound")]
    fn title_overflow_panics() {
        let too_long = vec![b'x'; 1024];
        let _ = ReleaseBuilder::new().title(&too_long);
    }
}
