//! Interactive builder for `Recording::V1`.

use anyhow::Result;
use midds_traits::{Isni, Isrc};
use midds_types::shared::{BPM_MAX, BPM_MIN, YEAR_MAX, YEAR_MIN};
use midds_types::{
    CONTRIBUTORS_MAX, Contributors, GENRES_MAX, Genre, Genres, PERFORMERS_MAX, PLACE_MAX_LEN,
    PRODUCERS_MAX, Performers, Producers, ProductionPlaces, Recording, RecordingV1,
    RecordingVersion, TITLE_ALIASES_MAX, TITLE_MAX_LEN, TitleAliases,
};

use crate::create::{prompts, shared};
use crate::ui;

const STEPS: usize = 10;

pub fn build() -> Result<Recording> {
    ui::section("Recording · V1");

    ui::step(1, STEPS, "Identification");
    let isrc: Isrc = prompts::identifier("ISRC", shared::parse_isrc_msg, "USRC17607839")?;
    let title = prompts::bounded_string::<TITLE_MAX_LEN>("Title", true)?;
    let title_aliases = build_title_aliases()?;

    ui::step(2, STEPS, "Artist & work");
    let artist = shared::party_id("Main artist identifier")?;
    let work = shared::work_ref("Recorded work")?;

    ui::step(3, STEPS, "Genres");
    let genres = build_genres()?;

    ui::step(4, STEPS, "Edition");
    let record_year = prompts::optional_int_in_range::<u16>("Record year", YEAR_MIN, YEAR_MAX)?;
    let version_type = prompts::optional("an edition / version type", || {
        prompts::fuzzy_select("Version type", VERSION_CHOICES)
    })?;

    ui::step(5, STEPS, "Performers");
    let performers = build_party_collection("performer", PERFORMERS_MAX as usize)?;
    let performers = Performers::try_from(performers)
        .map_err(|_| anyhow::anyhow!("more than {PERFORMERS_MAX} performers"))?;

    ui::step(6, STEPS, "Producers");
    let producers = build_producers()?;

    ui::step(7, STEPS, "Tempo & key");
    let duration = prompts::optional("the duration (whole seconds)", || {
        prompts::number::<u32>("Duration (s)", None)
    })?;
    let bpm = prompts::optional_int_in_range::<u16>("BPM", BPM_MIN, BPM_MAX)?;
    let key = prompts::optional("a musical key", || shared::musical_key("Musical key"))?;

    ui::step(8, STEPS, "Production places");
    let places = build_places()?;

    ui::step(9, STEPS, "Contributors");
    let contributors = build_party_collection("contributor", CONTRIBUTORS_MAX as usize)?;
    let contributors = Contributors::try_from(contributors)
        .map_err(|_| anyhow::anyhow!("more than {CONTRIBUTORS_MAX} contributors"))?;

    ui::step(10, STEPS, "Off-chain extension");
    let offchain_extension = shared::offchain_extension()?;

    Ok(Recording::V1(RecordingV1 {
        isrc,
        title,
        title_aliases,
        artist,
        work,
        genres,
        record_year,
        version_type,
        performers,
        producers,
        duration,
        bpm,
        key,
        places,
        contributors,
        offchain_extension,
    }))
}

const VERSION_CHOICES: &[(&str, RecordingVersion)] = &[
    ("Original", RecordingVersion::Original),
    ("Radio edit", RecordingVersion::RadioEdit),
    ("Extended", RecordingVersion::Extended),
    ("Remix", RecordingVersion::Remix),
    ("Live", RecordingVersion::Live),
    ("Acoustic", RecordingVersion::Acoustic),
    ("Instrumental", RecordingVersion::Instrumental),
    ("A cappella", RecordingVersion::ACapella),
    ("Karaoke", RecordingVersion::Karaoke),
    ("Demo", RecordingVersion::Demo),
    ("Re-recorded", RecordingVersion::ReRecorded),
    ("Edited", RecordingVersion::Edited),
    ("Cover", RecordingVersion::Cover),
];

const GENRE_CHOICES: &[(&str, Genre)] = &[
    ("Pop", Genre::Pop),
    ("Rock", Genre::Rock),
    ("Hip-Hop", Genre::HipHop),
    ("R&B", Genre::RnB),
    ("Electronic", Genre::Electronic),
    ("Dance", Genre::Dance),
    ("Jazz", Genre::Jazz),
    ("Blues", Genre::Blues),
    ("Classical", Genre::Classical),
    ("Country", Genre::Country),
    ("Folk", Genre::Folk),
    ("Metal", Genre::Metal),
    ("Punk", Genre::Punk),
    ("Reggae", Genre::Reggae),
    ("Latin", Genre::Latin),
    ("World", Genre::World),
    ("Soul", Genre::Soul),
    ("Funk", Genre::Funk),
    ("Gospel", Genre::Gospel),
    ("Soundtrack", Genre::Soundtrack),
    ("Ambient", Genre::Ambient),
    ("Experimental", Genre::Experimental),
    ("Children", Genre::Children),
    ("Spoken word", Genre::SpokenWord),
    ("Other", Genre::Other),
];

fn build_title_aliases() -> Result<TitleAliases> {
    let aliases = prompts::collect_bounded("title alias", 0, TITLE_ALIASES_MAX as usize, |_| {
        prompts::bounded_string::<TITLE_MAX_LEN>("Alias", true)
    })?;
    TitleAliases::try_from(aliases)
        .map_err(|_| anyhow::anyhow!("more than {TITLE_ALIASES_MAX} title aliases"))
}

fn build_genres() -> Result<Genres> {
    let genres = prompts::collect_bounded("genre", 0, GENRES_MAX as usize, |_| {
        prompts::fuzzy_select("Genre", GENRE_CHOICES)
    })?;
    Genres::try_from(genres).map_err(|_| anyhow::anyhow!("more than {GENRES_MAX} genres"))
}

/// Shared `Vec<PartyId>` collector for `performers` / `contributors` — both
/// are IPI-or-ISNI lists differing only in their cap and noun.
fn build_party_collection(noun: &str, max: usize) -> Result<Vec<midds_types::PartyId>> {
    prompts::collect_bounded(noun, 0, max, |_| {
        shared::party_id(&format!("{noun} identifier"))
    })
}

fn build_producers() -> Result<Producers> {
    let producers =
        prompts::collect_bounded("producer", 0, PRODUCERS_MAX as usize, |_| -> Result<Isni> {
            prompts::identifier("Producer ISNI", shared::parse_isni_msg, "0000000121032683")
        })?;
    Producers::try_from(producers)
        .map_err(|_| anyhow::anyhow!("more than {PRODUCERS_MAX} producers"))
}

fn build_places() -> Result<Option<ProductionPlaces>> {
    if !prompts::confirm(
        "Add production places (recording / mixing / mastering)?",
        false,
    )? {
        return Ok(None);
    }
    let recording = prompts::optional_string::<PLACE_MAX_LEN>("Recording place")?;
    let mixing = prompts::optional_string::<PLACE_MAX_LEN>("Mixing place")?;
    let mastering = prompts::optional_string::<PLACE_MAX_LEN>("Mastering place")?;
    Ok(Some(ProductionPlaces {
        recording,
        mixing,
        mastering,
    }))
}
