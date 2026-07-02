//! Interactive builder for `Release::V1`.

use anyhow::Result;
use midds_traits::{Isni, Upc};
use midds_types::release::{
    self, CATALOG_NUMBER_MAX_LEN as REL_CATALOG_MAX, COVER_CONTRIBUTOR_NAME_MAX_LEN,
    DISTRIBUTOR_NAME_MAX_LEN, FEATURING_MAX, Featuring,
};
use midds_types::{
    COVER_CONTRIBUTORS_MAX, CoverContributors, Producer, Release, ReleaseFormat, ReleasePackaging,
    ReleaseStatus, ReleaseType, ReleaseV1, TITLE_MAX_LEN, TRACKS_MAX, Track, Tracks,
};

use crate::create::{prompts, shared};
use crate::ui;

const STEPS: usize = 9;

/// Walk the interactive form and assemble a validated `Release::V1`.
pub fn build() -> Result<Release> {
    ui::section("Release · V1");

    ui::step(1, STEPS, "Identification");
    let upc: Upc = prompts::identifier("UPC / EAN", shared::parse_upc_msg, "0123456789012")?;
    let title = prompts::bounded_string::<TITLE_MAX_LEN>("Title", true)?;
    let title_aliases: release::TitleAliases =
        shared::title_aliases(release::TITLE_ALIASES_MAX as usize)?;

    ui::step(2, STEPS, "Artist & featuring");
    let artist = shared::party_id("Main artist identifier")?;
    let featuring: Featuring = shared::featuring(FEATURING_MAX as usize)?;

    ui::step(3, STEPS, "Tracklist");
    let tracks = build_tracks()?;

    ui::step(4, STEPS, "Producers");
    let producers = build_producers()?;

    ui::step(5, STEPS, "Editorial");
    let status = prompts::select(
        "Status",
        &[
            ("Official", ReleaseStatus::Official),
            ("Promotional", ReleaseStatus::Promotional),
            ("Bootleg", ReleaseStatus::Bootleg),
            ("Pseudo-release", ReleaseStatus::PseudoRelease),
            ("Withdrawn", ReleaseStatus::Withdrawn),
            ("Cancelled", ReleaseStatus::Cancelled),
            ("Other", ReleaseStatus::Other),
        ],
        0,
    )?;
    let release_type = prompts::fuzzy_select(
        "Release type",
        &[
            ("Album", ReleaseType::Album),
            ("Single", ReleaseType::Single),
            ("EP", ReleaseType::Ep),
            ("Broadcast", ReleaseType::Broadcast),
            ("Compilation", ReleaseType::Compilation),
            ("Soundtrack", ReleaseType::Soundtrack),
            ("Live", ReleaseType::Live),
            ("Remix", ReleaseType::Remix),
            ("Mixtape", ReleaseType::Mixtape),
            ("Demo", ReleaseType::Demo),
            ("Other", ReleaseType::Other),
        ],
    )?;

    ui::step(6, STEPS, "Release date & territory");
    let release_date = shared::release_date("Release date")?;
    let country = shared::country("Country of release")?;
    let distributor_name =
        prompts::bounded_string::<DISTRIBUTOR_NAME_MAX_LEN>("Distributor name", true)?;

    ui::step(7, STEPS, "Format & packaging");
    let format = prompts::fuzzy_select(
        "Format",
        &[
            ("CD", ReleaseFormat::Cd),
            ("Vinyl", ReleaseFormat::Vinyl),
            ("Cassette", ReleaseFormat::Cassette),
            ("Digital download", ReleaseFormat::DigitalDownload),
            ("Streaming", ReleaseFormat::Streaming),
            ("DVD", ReleaseFormat::Dvd),
            ("Blu-ray", ReleaseFormat::BluRay),
            ("SACD", ReleaseFormat::Sacd),
            ("MiniDisc", ReleaseFormat::MiniDisc),
            ("Reel-to-reel", ReleaseFormat::ReelToReel),
            ("Other", ReleaseFormat::Other),
        ],
    )?;
    let packaging = prompts::fuzzy_select(
        "Packaging",
        &[
            ("None (digital)", ReleasePackaging::None),
            ("Jewel case", ReleasePackaging::JewelCase),
            ("Slim jewel case", ReleasePackaging::SlimJewelCase),
            ("Digipak", ReleasePackaging::Digipak),
            ("Cardboard sleeve", ReleasePackaging::CardboardSleeve),
            ("Gatefold", ReleasePackaging::Gatefold),
            ("Keep case", ReleasePackaging::KeepCase),
            ("Box", ReleasePackaging::Box),
            ("Other", ReleasePackaging::Other),
        ],
    )?;

    ui::step(8, STEPS, "Cover contributors");
    let cover_contributors = build_cover_contributors()?;

    ui::step(9, STEPS, "Off-chain extension");
    let offchain_extension = shared::offchain_extension()?;

    Ok(Release::V1(ReleaseV1 {
        upc,
        title,
        title_aliases,
        artist,
        featuring,
        tracks,
        producers,
        status,
        release_date,
        country,
        distributor_name,
        release_type,
        format,
        packaging,
        cover_contributors,
        offchain_extension,
    }))
}

fn build_tracks() -> Result<Tracks> {
    ui::info(
        "a release needs at least one track; track numbers must run 1..N \
         (start at 1, no gaps) and every recording must be unique",
    );
    prompts::collect_bounded_into("track", 1, TRACKS_MAX as usize, |idx| -> Result<Track> {
        let number = prompts::int_in_range::<u16>(
            "Track number",
            1,
            TRACKS_MAX as u16,
            Some((idx + 1) as u16),
        )?;
        let recording = shared::recording_ref(&format!("Track #{number} recording"))?;
        Ok(Track { number, recording })
    })
}

fn build_producers() -> Result<release::Producers> {
    prompts::collect_bounded_into(
        "producer / label",
        0,
        release::PRODUCERS_MAX as usize,
        |_| -> Result<Producer> {
            let isni: Isni =
                prompts::identifier("Producer ISNI", shared::parse_isni_msg, "0000000121032683")?;
            let catalog_number =
                prompts::bounded_string::<REL_CATALOG_MAX>("Catalogue number", true)?;
            Ok(Producer {
                isni,
                catalog_number,
            })
        },
    )
}

fn build_cover_contributors() -> Result<CoverContributors> {
    prompts::collect_bounded_into(
        "cover contributor",
        0,
        COVER_CONTRIBUTORS_MAX as usize,
        |_| prompts::bounded_string::<COVER_CONTRIBUTOR_NAME_MAX_LEN>("Contributor name", true),
    )
}
