//! Interactive builder for `Recording::V1`.

use anyhow::Result;
use midds_traits::{Isni, Isrc};
use midds_types::shared::{BPM_MAX, BPM_MIN, YEAR_MAX, YEAR_MIN};
use midds_types::{
    CONTRIBUTORS_MAX, Contributors, FEATURING_MAX, Featuring, GENRES_MAX, Genre, Genres,
    INSTRUMENTS_PER_PERFORMER_MAX, Instrument, PERFORMERS_MAX, PLACE_MAX_LEN, PRODUCERS_MAX,
    Performer, PerformerInstruments, Performers, Producers, ProductionPlaces, Recording,
    RecordingV1, RecordingVersion, TITLE_ALIASES_MAX, TITLE_MAX_LEN, TitleAliases,
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

    ui::step(2, STEPS, "Artist, featuring & work");
    let artist = shared::party_id("Main artist identifier")?;
    let featuring = build_featuring()?;
    let work = shared::work_ref("Recorded work")?;

    ui::step(3, STEPS, "Genres");
    let genres = build_genres()?;
    let sub_genre = prompts::optional("a sub-genre", || {
        prompts::fuzzy_select("Sub-genre", GENRE_CHOICES)
    })?;

    ui::step(4, STEPS, "Edition");
    let record_year = prompts::optional_int_in_range::<u16>("Record year", YEAR_MIN, YEAR_MAX)?;
    let version_type = prompts::optional("an edition / version type", || {
        prompts::fuzzy_select("Version type", VERSION_CHOICES)
    })?;

    ui::step(5, STEPS, "Performers");
    let performers = build_performer_collection(PERFORMERS_MAX as usize)?;
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
        featuring,
        work,
        genres,
        sub_genre,
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
    ("Clean", RecordingVersion::Clean),
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

const INSTRUMENT_CHOICES: &[(&str, Instrument)] = &[
    // Vocals
    ("Vocals", Instrument::Vocals),
    ("Lead vocals", Instrument::LeadVocals),
    ("Backing vocals", Instrument::BackingVocals),
    ("Choir", Instrument::Choir),
    // Keyboards
    ("Piano", Instrument::Piano),
    ("Electric piano", Instrument::ElectricPiano),
    ("Organ", Instrument::Organ),
    ("Harpsichord", Instrument::Harpsichord),
    ("Synthesizer", Instrument::Synthesizer),
    ("Accordion", Instrument::Accordion),
    ("Celesta", Instrument::Celesta),
    ("Melodica", Instrument::Melodica),
    ("Keyboards", Instrument::Keyboards),
    // Plucked strings
    ("Acoustic guitar", Instrument::AcousticGuitar),
    ("Electric guitar", Instrument::ElectricGuitar),
    ("Bass guitar", Instrument::BassGuitar),
    ("Classical guitar", Instrument::ClassicalGuitar),
    ("Guitar", Instrument::Guitar),
    ("Banjo", Instrument::Banjo),
    ("Mandolin", Instrument::Mandolin),
    ("Ukulele", Instrument::Ukulele),
    ("Harp", Instrument::Harp),
    ("Sitar", Instrument::Sitar),
    ("Lute", Instrument::Lute),
    ("Balalaika", Instrument::Balalaika),
    ("Oud", Instrument::Oud),
    // Bowed strings
    ("Violin", Instrument::Violin),
    ("Viola", Instrument::Viola),
    ("Cello", Instrument::Cello),
    ("Double bass", Instrument::DoubleBass),
    ("Strings", Instrument::Strings),
    // Woodwinds
    ("Flute", Instrument::Flute),
    ("Piccolo", Instrument::Piccolo),
    ("Clarinet", Instrument::Clarinet),
    ("Oboe", Instrument::Oboe),
    ("Bassoon", Instrument::Bassoon),
    ("Saxophone", Instrument::Saxophone),
    ("Recorder", Instrument::Recorder),
    ("English horn", Instrument::EnglishHorn),
    ("Harmonica", Instrument::Harmonica),
    ("Bagpipes", Instrument::Bagpipes),
    ("Pan flute", Instrument::PanFlute),
    // Brass
    ("Trumpet", Instrument::Trumpet),
    ("Cornet", Instrument::Cornet),
    ("Flugelhorn", Instrument::Flugelhorn),
    ("Trombone", Instrument::Trombone),
    ("French horn", Instrument::FrenchHorn),
    ("Tuba", Instrument::Tuba),
    ("Euphonium", Instrument::Euphonium),
    // Pitched / mallet percussion
    ("Marimba", Instrument::Marimba),
    ("Xylophone", Instrument::Xylophone),
    ("Vibraphone", Instrument::Vibraphone),
    ("Glockenspiel", Instrument::Glockenspiel),
    ("Timpani", Instrument::Timpani),
    ("Steelpan", Instrument::Steelpan),
    // Percussion & drums
    ("Drum kit", Instrument::DrumKit),
    ("Snare drum", Instrument::SnareDrum),
    ("Bass drum", Instrument::BassDrum),
    ("Cymbals", Instrument::Cymbals),
    ("Tambourine", Instrument::Tambourine),
    ("Congas", Instrument::Congas),
    ("Bongos", Instrument::Bongos),
    ("Djembe", Instrument::Djembe),
    ("Tabla", Instrument::Tabla),
    ("Cajon", Instrument::Cajon),
    ("Triangle", Instrument::Triangle),
    ("Castanets", Instrument::Castanets),
    ("Maracas", Instrument::Maracas),
    ("Timbales", Instrument::Timbales),
    ("Cowbell", Instrument::Cowbell),
    ("Hand claps", Instrument::HandClaps),
    ("Percussion", Instrument::Percussion),
    // Electronic / production
    ("Drum machine", Instrument::DrumMachine),
    ("Sampler", Instrument::Sampler),
    ("Sequencer", Instrument::Sequencer),
    ("Turntables", Instrument::Turntables),
    ("Theremin", Instrument::Theremin),
    // Fallback
    ("Other", Instrument::Other),
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

/// `Vec<PartyId>` collector for `contributors`. The performer collector is
/// separate ([`build_performer_collection`]) because performers use the wider
/// [`midds_types::PerformerId`] enum (IPN / IPI / ISNI).
fn build_party_collection(noun: &str, max: usize) -> Result<Vec<midds_types::PartyId>> {
    prompts::collect_bounded(noun, 0, max, |_| {
        shared::party_id(&format!("{noun} identifier"))
    })
}

/// `Vec<Performer>` collector for the `performers` list. Distinct from
/// [`build_party_collection`] because the performer identifier set carries
/// IPN as its primary code, and each performer additionally lists the
/// instrument(s) they played.
fn build_performer_collection(max: usize) -> Result<Vec<Performer>> {
    prompts::collect_bounded("performer", 0, max, |_| build_performer())
}

/// One performer: a performer identifier (IPN / IPI / ISNI) followed by the
/// instrument(s) they played.
fn build_performer() -> Result<Performer> {
    let id = shared::performer_id("performer identifier")?;
    let instruments = build_instruments()?;
    Ok(Performer { id, instruments })
}

/// The instrument(s) one performer played (0..=`INSTRUMENTS_PER_PERFORMER_MAX`,
/// type-to-filter over the full taxonomy).
fn build_instruments() -> Result<PerformerInstruments> {
    let instruments = prompts::collect_bounded(
        "instrument",
        0,
        INSTRUMENTS_PER_PERFORMER_MAX as usize,
        |_| prompts::fuzzy_select("Instrument", INSTRUMENT_CHOICES),
    )?;
    PerformerInstruments::try_from(instruments)
        .map_err(|_| anyhow::anyhow!("more than {INSTRUMENTS_PER_PERFORMER_MAX} instruments"))
}

/// `Featuring` collector. Featured artists carry the same
/// [`midds_types::PartyId`] as the main artist, so this reuses
/// [`build_party_collection`].
fn build_featuring() -> Result<Featuring> {
    let featuring = build_party_collection("featured artist", FEATURING_MAX as usize)?;
    Featuring::try_from(featuring)
        .map_err(|_| anyhow::anyhow!("more than {FEATURING_MAX} featured artists"))
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
