//! Interactive builder for `Recording::V1`.

use anyhow::Result;
use midds_traits::{Isni, Isrc};
use midds_types::shared::{BPM_MAX, BPM_MIN, YEAR_MAX, YEAR_MIN};
use midds_types::{
    CONTRIBUTORS_MAX, Contributors, FEATURING_MAX, Featuring, Genre, INSTRUMENTS_PER_PERFORMER_MAX,
    Instrument, PERFORMERS_MAX, PLACE_MAX_LEN, PRODUCERS_MAX, Performer, PerformerInstruments,
    Performers, Producers, ProductionPlaces, Recording, RecordingV1, RecordingVersion,
    TITLE_ALIASES_MAX, TITLE_MAX_LEN, TitleAliases,
};

use crate::create::{prompts, shared};
use crate::ui;

const STEPS: usize = 9;

/// Walk the interactive form and assemble a validated `Recording::V1`.
pub fn build() -> Result<Recording> {
    ui::section("Recording · V1");

    ui::step(1, STEPS, "Identification");
    let isrc: Isrc = prompts::identifier("ISRC", shared::parse_isrc_msg, "USRC17607839")?;
    let title = prompts::bounded_string::<TITLE_MAX_LEN>("Title", true)?;
    let title_aliases: TitleAliases = shared::title_aliases(TITLE_ALIASES_MAX as usize)?;

    ui::step(2, STEPS, "Artist, featuring & work");
    let artist = shared::party_id("Main artist identifier")?;
    let featuring: Featuring = shared::featuring(FEATURING_MAX as usize)?;
    let work = shared::work_ref("Recorded work")?;

    ui::step(3, STEPS, "Genre");
    let genre = prompts::optional("a genre", || prompts::fuzzy_select("Genre", GENRE_CHOICES))?;
    // A sub-genre refines a primary genre, so it is only offered once one is
    // set (`validate_format` rejects a lone sub-genre).
    let sub_genre = if genre.is_some() {
        prompts::optional("a sub-genre", || {
            prompts::fuzzy_select("Sub-genre", GENRE_CHOICES)
        })?
    } else {
        None
    };

    ui::step(4, STEPS, "Edition");
    let record_year = prompts::optional_int_in_range::<u16>("Record year", YEAR_MIN, YEAR_MAX)?;
    let version_type = prompts::optional("an edition / version type", || {
        prompts::fuzzy_select("Version type", VERSION_CHOICES)
    })?;

    ui::step(5, STEPS, "Performers");
    let performers: Performers =
        prompts::collect_bounded_into("performer", 0, PERFORMERS_MAX as usize, |_| {
            build_performer()
        })?;

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
    let contributors: Contributors =
        prompts::collect_bounded_into("contributor", 0, CONTRIBUTORS_MAX as usize, |_| {
            shared::party_id("contributor identifier")
        })?;

    Ok(Recording::V1(RecordingV1 {
        isrc,
        title,
        title_aliases,
        artist,
        featuring,
        work,
        genre,
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

/// One performer: a performer identifier (IPN / IPI / ISNI) followed by the
/// instrument(s) they played.
fn build_performer() -> Result<Performer> {
    let id = shared::performer_id("performer identifier")?;
    let instruments = build_instruments()?;
    Ok(Performer { id, instruments })
}

/// The instrument(s) one performer played (1..=`INSTRUMENTS_PER_PERFORMER_MAX`,
/// type-to-filter over the full taxonomy). At least one is mandatory: once a
/// performer id has been entered, its instrument must be filled in.
fn build_instruments() -> Result<PerformerInstruments> {
    prompts::collect_bounded_into(
        "instrument",
        1,
        INSTRUMENTS_PER_PERFORMER_MAX as usize,
        |_| prompts::fuzzy_select("Instrument", INSTRUMENT_CHOICES),
    )
}

fn build_producers() -> Result<Producers> {
    prompts::collect_bounded_into("producer", 0, PRODUCERS_MAX as usize, |_| -> Result<Isni> {
        prompts::identifier("Producer ISNI", shared::parse_isni_msg, "0000000121032683")
    })
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
