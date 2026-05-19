//! Offline `create` command: build a MIDDS payload through a guided form,
//! validate it with the on-chain `validate_format`, and emit the canonical
//! SCALE hex and/or JSON. **Never connects to a node** — this is purely a
//! payload factory; depositing stays the job of `midds-client`.

mod musical_work;
mod prompts;
mod recording;
mod release;
mod shared;

use std::path::PathBuf;

use anyhow::{Context, Result};
// `Midds: Parameter: Codec` already brings `Encode::encode` into scope for any
// `M: Midds`, so no explicit `parity_scale_codec::Encode` import is needed.
use midds_traits::Midds;

use crate::cli::{CreateFormat, MiddsKind};
use crate::{format, ui};

/// Entry point for `midds create`. `kind` is the `--type` value (prompted when
/// absent); `out` writes the payload to a file instead of stdout; `fmt`
/// selects the representation.
pub fn run(kind: Option<MiddsKind>, out: Option<PathBuf>, fmt: CreateFormat) -> Result<()> {
    let kind = match kind {
        Some(k) => k,
        None => prompts::select(
            "Which MIDDS do you want to create?",
            &[
                (
                    "MusicalWork — a composition (ISWC-keyed)",
                    MiddsKind::MusicalWork,
                ),
                (
                    "Recording — a recorded performance (ISRC-keyed)",
                    MiddsKind::Recording,
                ),
                (
                    "Release — a commercial release (UPC/EAN-keyed)",
                    MiddsKind::Release,
                ),
            ],
            0,
        )?,
    };

    // Each arm builds its concrete type, re-prompting on a failed
    // `validate_format`, then hands off to the generic emitter.
    match kind {
        MiddsKind::MusicalWork => finish(build_validated(musical_work::build)?, &out, fmt),
        MiddsKind::Recording => finish(build_validated(recording::build)?, &out, fmt),
        MiddsKind::Release => finish(build_validated(release::build)?, &out, fmt),
    }
}

/// Run `build`, then the on-chain `validate_format`. Field prompts already
/// reject most invalids inline, so a failure here is almost always a
/// cross-field rule (medley < 2 sources, duplicate tracklist entry); show it
/// and let the user re-enter the form rather than losing the whole session.
fn build_validated<M: Midds>(build: fn() -> Result<M>) -> Result<M> {
    loop {
        let payload = build()?;
        match payload.validate_format() {
            Ok(()) => {
                ui::success("payload passes on-chain validate_format");
                return Ok(payload);
            }
            Err(e) => {
                ui::error_line(&format!("validate_format rejected the payload: {e:?}"));
                ui::info(explain(&format!("{e:?}")));
                if !prompts::confirm("Re-enter the form?", true)? {
                    anyhow::bail!("aborted: payload did not pass validate_format");
                }
            }
        }
    }
}

/// One-line operator hint for the cross-field rules a field-by-field wizard
/// can still trip, keyed off the `MiddsFormatError` debug name.
fn explain(err: &str) -> &'static str {
    match err {
        "CrossFieldInconsistency" => {
            "a recording appears twice in the tracklist — every track must be unique"
        }
        "OutOfBounds" => {
            "a medley/mashup needs at least 2 source works, or a value is out of range"
        }
        "EmptyMandatoryField" => "a mandatory field was left empty",
        _ => "see docs/validation.md for the per-field rules",
    }
}

/// SCALE-encode + JSON-serialise the validated payload and route it to a file
/// or stdout. Chrome goes to stderr (`ui::*`), the payload to stdout
/// (`ui::payload_panel`) so redirects stay clean.
fn finish<M: Midds + serde::Serialize>(
    payload: M,
    out: &Option<PathBuf>,
    fmt: CreateFormat,
) -> Result<()> {
    const HEX_LABEL: &str = "SCALE hex (no 0x prefix)";
    const JSON_LABEL: &str = "JSON";

    let bytes = payload.encode();
    let hex = format::hex(&bytes);
    let json = serde_json::to_string_pretty(&payload).context("serialise payload to JSON")?;

    ui::summary(
        "Built MIDDS",
        &[
            ("Type".into(), M::KIND.into()),
            ("Encoded size".into(), format!("{} bytes", bytes.len())),
            ("Validation".into(), "passed".into()),
        ],
    );

    match out {
        Some(path) => {
            // With `--out`, write a single canonical form: JSON by default,
            // hex on explicit `--format hex`. Echo the complementary form to
            // stderr so a file run still surfaces both without polluting the
            // written artifact.
            let (file_label, file_body, echo_label, echo_body) = if matches!(fmt, CreateFormat::Hex)
            {
                (HEX_LABEL, &hex, JSON_LABEL, &json)
            } else {
                (JSON_LABEL, &json, HEX_LABEL, &hex)
            };
            std::fs::write(path, file_body)
                .with_context(|| format!("write payload to {}", path.display()))?;
            ui::success(&format!(
                "{file_label} payload written to {}",
                path.display()
            ));
            ui::info(&format!("{echo_label}: {echo_body}"));
        }
        None => match fmt {
            CreateFormat::Json => ui::payload_panel(JSON_LABEL, &json),
            CreateFormat::Hex => ui::payload_panel(HEX_LABEL, &hex),
            CreateFormat::Both => {
                ui::payload_panel(HEX_LABEL, &hex);
                ui::payload_panel(JSON_LABEL, &json);
            }
        },
    }
    Ok(())
}
