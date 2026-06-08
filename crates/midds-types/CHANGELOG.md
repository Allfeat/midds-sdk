# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-types-v0.2.0) - 2026-06-08

### Added

- *(midds-types)* [**breaking**] add cross-natural enharmonics to PitchClass
- *(midds-types)* [**breaking**] replace Recording genres list with a single optional genre
- *(midds-types)* [**breaking**] add featuring and numbered tracks to Release V1
- *(midds-types)* [**breaking**] extend Recording V1 with featuring, sub_genre, instruments and Clean version
- *(midds-types)* [**breaking**] add explicit_lyrics, sampled-work refs, and Rearrangement to MusicalWork
- *(midds-types)* reject self-referencing and duplicate work references
- *(midds-types)* [**breaking**] introduce PerformerId (IPN/IPI/ISNI) for Recording.performers
- *(midds-types)* [**breaking**] add flat spellings to PitchClass
- *(midds-types)* [**breaking**] make MusicalWork.creation_year optional
- *(midds-types)* [**breaking**] creator carries multiple roles and dual IPI+ISNI identifiers
- *(midds-types)* stabilize V1 per-field validation rules
- add Release MIDDS type (UPC/EAN-keyed, third and final V1 type)
- *(midds-types)* [**breaking**] add Recording MIDDS type and shared cross-type module
- canonical JSON wire format for MIDDS payloads
- bundle in-flight v1-hardening work + pallet module split

### Fixed

- *(midds-traits, midds-types)* propagate serde feature to bounded-collections

### Other

- *(midds-types)* O(n log n) Release tracklist dedup via BTreeSet
- drop non-doc inline comments across the workspace
- *(midds-types)* add Recording encoding proptest invariants
- *(midds-traits, midds-types)* drop frame-support for browser WASM compat
- release v0.1.0
- initial commit

## [0.1.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-types-v0.1.0) - 2026-05-04

### Added

- bundle in-flight v1-hardening work + pallet module split

### Other

- initial commit
