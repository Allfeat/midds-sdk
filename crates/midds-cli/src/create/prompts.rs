//! Reusable, typed input widgets for the offline `create` wizard.
//!
//! Every MIDDS field shape the three builders need is reduced to one helper
//! here so the per-type forms stay declarative and the validation feedback is
//! identical everywhere. All prompts go through the shared [`ui::theme`] and
//! write to stderr; nothing here touches a node.
//!
//! The on-chain `validate_format` is still the source of truth — these helpers
//! reject the obvious (empty mandatory text, out-of-range numbers, malformed
//! identifiers) *as you type* so the final validation pass almost never fails,
//! but the builders re-run `validate_format` regardless.

use std::fmt::Display;
use std::str::FromStr;

use anyhow::{Context, Result};
use dialoguer::{Confirm, FuzzySelect, Input, MultiSelect, Select};
use midds_traits::MiddsString;

use crate::ui;

/// Free-text field backed by a `MiddsString<N>` (byte-length-bounded).
///
/// `required` rejects an empty value (mirrors the `EmptyMandatoryField`
/// `validate_format` rule for mandatory text). The length cap is enforced on
/// the raw UTF-8 byte count, exactly like the on-chain `BoundedVec` bound.
pub fn bounded_string<const N: u32>(label: &str, required: bool) -> Result<MiddsString<N>> {
    let max = N as usize;
    let input = Input::<String>::with_theme(&ui::theme())
        .with_prompt(label)
        .allow_empty(!required)
        .validate_with(move |s: &String| -> Result<(), String> {
            if required && s.trim().is_empty() {
                return Err("required — cannot be empty".into());
            }
            if s.len() > max {
                return Err(format!("{} bytes, exceeds the {max}-byte bound", s.len()));
            }
            Ok(())
        })
        .interact_text()
        .with_context(|| format!("read `{label}`"))?;
    MiddsString::<N>::try_from(input.into_bytes())
        .map_err(|_| anyhow::anyhow!("`{label}` exceeds its {max}-byte bound"))
}

/// Wrap any prompt as `Option<T>` behind a `set this?` gate.
///
/// Every optional MIDDS field — language, key, classical metadata, off-chain
/// hash, … — follows the same shape: a `Confirm`, then the inner prompt only
/// if the user opted in. Centralising the pattern keeps each builder
/// declarative (one line per field).
pub fn optional<T>(label: &str, build: impl FnOnce() -> Result<T>) -> Result<Option<T>> {
    if !confirm(&format!("Set {label}?"), false)? {
        return Ok(None);
    }
    Ok(Some(build()?))
}

/// Optional free-text field: a `set this?` gate, then [`bounded_string`].
pub fn optional_string<const N: u32>(label: &str) -> Result<Option<MiddsString<N>>> {
    optional(label, || bounded_string::<N>(label, true))
}

/// An industry identifier (ISWC / ISNI / IPI / ISRC / UPC / off-chain hash).
///
/// `parse` is the matching tolerant parser from `midds-validate`, wrapped to
/// emit a human-readable rule on rejection (e.g. `"ISNI must be 16 characters
/// — 15 digits + a final digit or X"`). The prompt loops until the parser
/// accepts the input, so a built MIDDS never carries a structurally invalid
/// identifier *and* the user sees what shape the field expects instead of
/// the raw `MiddsFormatError` variant. `example` is shown as a hint.
///
/// Returning the parser's normalised output (not the raw `MiddsString<N>`
/// bytes) is what lets the IPI prompt accept `"1"` and submit `"00000000001"`
/// without the call site needing to know about padding.
pub fn identifier<T>(
    label: &str,
    parse: impl Fn(&str) -> Result<T, String> + Clone + 'static,
    example: &str,
) -> Result<T> {
    identifier_capped(label, parse, example, usize::MAX)
}

/// Like [`identifier`] but rejects raw input longer than `max_chars` before the
/// parser runs. For fixed-width identifiers (e.g. ISRC = 12) so an over-long
/// paste is refused up front with a clear message rather than slipping through
/// the tolerant parser (which strips separators) or being silently truncated.
pub fn identifier_capped<T>(
    label: &str,
    parse: impl Fn(&str) -> Result<T, String> + Clone + 'static,
    example: &str,
    max_chars: usize,
) -> Result<T> {
    let parse_for_validate = parse.clone();
    let raw = Input::<String>::with_theme(&ui::theme())
        .with_prompt(format!("{label} (e.g. {example})"))
        .validate_with(move |s: &String| -> Result<(), String> {
            if s.trim().chars().count() > max_chars {
                return Err(format!("at most {max_chars} characters"));
            }
            parse_for_validate(s.as_str()).map(|_| ())
        })
        .interact_text()
        .with_context(|| format!("read `{label}`"))?;
    parse(&raw).map_err(|e| anyhow::anyhow!("`{label}`: {e}"))
}

/// A number constrained to an inclusive `[min, max]` range. Used for every
/// year / bpm / month / day / duration / voice-count field; the range is the
/// one `validate_format` enforces for that field.
pub fn int_in_range<T>(label: &str, min: T, max: T, default: Option<T>) -> Result<T>
where
    T: Copy + PartialOrd + Display + FromStr + Clone + 'static,
    <T as FromStr>::Err: Display,
{
    let theme = ui::theme();
    let mut builder = Input::<T>::with_theme(&theme)
        .with_prompt(format!("{label} [{min}..={max}]"))
        .validate_with(move |n: &T| -> Result<(), String> {
            if *n < min || *n > max {
                Err(format!("must be within {min}..={max}"))
            } else {
                Ok(())
            }
        });
    if let Some(d) = default {
        builder = builder.default(d);
    }
    builder
        .interact_text()
        .with_context(|| format!("read `{label}`"))
}

/// An unconstrained number (no range bracket in the prompt). For fields the
/// spec deliberately leaves uncapped — `Release.release_date.year`, on-chain
/// MIDDS ids — where an `[lo..=hi]` hint would be noise.
pub fn number<T>(label: &str, default: Option<T>) -> Result<T>
where
    T: Clone + ToString + FromStr,
    <T as FromStr>::Err: ToString,
{
    let theme = ui::theme();
    let mut builder = Input::<T>::with_theme(&theme).with_prompt(label);
    if let Some(d) = default {
        builder = builder.default(d);
    }
    builder
        .interact_text()
        .with_context(|| format!("read `{label}`"))
}

/// A `u64` on-chain MIDDS id (`WorkRef::Midds` / `RecordingRef::Midds`).
///
/// The raw input is capped at 12 digits: MIDDS ids are allocated sequentially
/// and stay far below that for the foreseeable future, and the cap keeps a long
/// paste from overflowing `u64` — which would otherwise surface as dialoguer's
/// raw "number too large to fit in target type" instead of a clear rule.
pub fn midds_id(label: &str) -> Result<u64> {
    let raw = Input::<String>::with_theme(&ui::theme())
        .with_prompt(label)
        .validate_with(|s: &String| -> Result<(), String> {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return Err("required — enter a MIDDS id".into());
            }
            if !trimmed.bytes().all(|b| b.is_ascii_digit()) {
                return Err("digits only".into());
            }
            if trimmed.len() > 12 {
                return Err("at most 12 digits".into());
            }
            Ok(())
        })
        .interact_text()
        .with_context(|| format!("read `{label}`"))?;
    raw.trim()
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("`{label}`: {e}"))
}

/// Optional ranged number: a `set this?` gate, then [`int_in_range`].
pub fn optional_int_in_range<T>(label: &str, min: T, max: T) -> Result<Option<T>>
where
    T: Copy + PartialOrd + Display + FromStr + Clone + 'static,
    <T as FromStr>::Err: Display,
{
    optional(label, || int_in_range(label, min, max, None))
}

/// Yes/No prompt with an explicit default.
pub fn confirm(label: &str, default: bool) -> Result<bool> {
    Confirm::with_theme(&ui::theme())
        .with_prompt(label)
        .default(default)
        .interact()
        .with_context(|| format!("read `{label}`"))
}

/// Single-choice picker over labelled variants (short lists: enum tags,
/// IPI-vs-ISNI, …). Returns the chosen `T` by value.
pub fn select<T: Clone>(label: &str, choices: &[(&str, T)], default: usize) -> Result<T> {
    let labels: Vec<&str> = choices.iter().map(|(l, _)| *l).collect();
    let idx = Select::with_theme(&ui::theme())
        .with_prompt(label)
        .items(&labels)
        .default(default.min(choices.len().saturating_sub(1)))
        .interact()
        .with_context(|| format!("pick `{label}`"))?;
    Ok(choices[idx].1.clone())
}

/// Multi-choice picker over labelled variants. Returns the chosen `T`s in
/// menu order. Re-prompts until the user picks at least `min` entries —
/// mandatory collections (e.g. a creator's roles) refuse to be empty.
pub fn multi_select<T: Clone>(label: &str, choices: &[(&str, T)], min: usize) -> Result<Vec<T>> {
    let labels: Vec<&str> = choices.iter().map(|(l, _)| *l).collect();
    loop {
        let picks: Vec<usize> = MultiSelect::with_theme(&ui::theme())
            .with_prompt(label)
            .items(&labels)
            .interact()
            .with_context(|| format!("pick `{label}`"))?;
        if picks.len() < min {
            ui::info(&format!("at least {min} item(s) required"));
            continue;
        }
        return Ok(picks.into_iter().map(|i| choices[i].1.clone()).collect());
    }
}

/// Type-to-filter picker for the longer closed enums (Genre, …). Same
/// contract as [`select`] but backed by `dialoguer`'s fuzzy matcher.
pub fn fuzzy_select<T: Clone>(label: &str, choices: &[(&str, T)]) -> Result<T> {
    let labels: Vec<&str> = choices.iter().map(|(l, _)| *l).collect();
    let idx = FuzzySelect::with_theme(&ui::theme())
        .with_prompt(label)
        .items(&labels)
        .default(0)
        .interact()
        .with_context(|| format!("pick `{label}`"))?;
    Ok(choices[idx].1.clone())
}

/// Build a homogeneous, cardinality-bounded collection.
///
/// Returns a plain `Vec<T>`; the caller wraps it in the concrete
/// `BoundedVec` alias (`Creators`, `Tracks`, …) so the on-chain `MAX` is
/// enforced once, by the type, with the canonical error. `min` is enforced
/// here (a non-empty mandatory collection refuses to finish empty); `max`
/// stops offering "add another" at the cap so the wrap can never fail.
/// [`collect_bounded`] wrapped into the concrete `BoundedVec` alias. The
/// loop stops at `max`, so the final `try_from` is a defensive path only.
pub fn collect_bounded_into<T, B>(
    noun: &str,
    min: usize,
    max: usize,
    build: impl FnMut(usize) -> Result<T>,
) -> Result<B>
where
    B: TryFrom<Vec<T>>,
{
    let items = collect_bounded(noun, min, max, build)?;
    B::try_from(items).map_err(|_| anyhow::anyhow!("more than {max} {noun}s"))
}

pub fn collect_bounded<T>(
    noun: &str,
    min: usize,
    max: usize,
    mut build: impl FnMut(usize) -> Result<T>,
) -> Result<Vec<T>> {
    let mut items = Vec::new();
    loop {
        let idx = items.len();
        if idx >= max {
            ui::info(&format!("reached the {max}-{noun} cap"));
            break;
        }
        let need_more = idx < min;
        let prompt = if idx == 0 {
            format!("Add a {noun}?")
        } else {
            format!("Add another {noun}? ({idx} added)")
        };
        let proceed = if need_more {
            if idx == 0 && min == 1 {
                true
            } else {
                ui::info(&format!("at least {min} {noun}(s) required"));
                true
            }
        } else {
            confirm(&prompt, false)?
        };
        if !proceed {
            break;
        }
        ui::hint(&format!("{noun} #{}", idx + 1));
        items.push(build(idx).with_context(|| format!("build {noun} #{}", idx + 1))?);
    }
    Ok(items)
}
