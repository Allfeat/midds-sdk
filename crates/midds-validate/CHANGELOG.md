# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-validate-v0.2.0) - 2026-06-08

### Added

- *(midds-validate)* allow explicit track numbers in ReleaseBuilder
- *(midds-types)* [**breaking**] replace Recording genres list with a single optional genre
- *(midds-types)* [**breaking**] add featuring and numbered tracks to Release V1
- *(midds-types)* [**breaking**] extend Recording V1 with featuring, sub_genre, instruments and Clean version
- *(midds-types)* [**breaking**] add explicit_lyrics, sampled-work refs, and Rearrangement to MusicalWork
- *(midds-types)* [**breaking**] introduce PerformerId (IPN/IPI/ISNI) for Recording.performers
- *(midds-validate)* pad short IPIs to 11 digits in `parse_ipi`
- *(midds-types)* [**breaking**] make MusicalWork.creation_year optional
- *(midds-types)* [**breaking**] creator carries multiple roles and dual IPI+ISNI identifiers
- add Release MIDDS type (UPC/EAN-keyed, third and final V1 type)
- *(midds-validate)* add parser-tolerant RecordingBuilder
- bundle in-flight v1-hardening work + pallet module split

### Other

- drop non-doc inline comments across the workspace
- *(midds-validate)* drop frame-support for browser WASM compat
- release v0.1.0
- initial commit

## [0.1.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-validate-v0.1.0) - 2026-05-04

### Added

- bundle in-flight v1-hardening work + pallet module split

### Other

- initial commit
