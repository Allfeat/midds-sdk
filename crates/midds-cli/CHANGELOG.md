# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-cli-v0.2.0) - 2026-06-08

### Added

- *(midds-types)* [**breaking**] add cross-natural enharmonics to PitchClass
- *(midds-types)* [**breaking**] replace Recording genres list with a single optional genre
- *(midds-types)* [**breaking**] add featuring and numbered tracks to Release V1
- *(midds-types)* [**breaking**] extend Recording V1 with featuring, sub_genre, instruments and Clean version
- *(midds-types)* [**breaking**] add explicit_lyrics, sampled-work refs, and Rearrangement to MusicalWork
- *(midds-types)* [**breaking**] introduce PerformerId (IPN/IPI/ISNI) for Recording.performers
- *(midds-cli)* surface human-readable identifier rules in `create` wizard
- *(midds-types)* [**breaking**] add flat spellings to PitchClass
- *(midds-types)* [**breaking**] make MusicalWork.creation_year optional
- *(midds-types)* [**breaking**] creator carries multiple roles and dual IPI+ISNI identifiers
- *(pallet-midds)* [**breaking**] make `DepositBase` / `DepositPerByte` runtime-mutable
- *(midds-cli)* production-grade UX overhaul + offline `create` wizard
- add Release MIDDS type (UPC/EAN-keyed, third and final V1 type)
- bundle in-flight v1-hardening work + pallet module split

### Fixed

- *(midds-cli)* exit non-zero from `seed` when deposits fail
- *(midds-client)* [**breaking**] read at the best block instead of the finalised one

### Other

- sync pallet docs with implementation; document sponsor-premium exposure
- drop non-doc inline comments across the workspace
- *(midds-cli)* factor `optional` / `offchain_extension` / `auto_fund` helpers
- *(midds-cli)* make the bench harness generic over the MIDDS type
- release v0.1.0
- apply rustfmt
- initial commit

## [0.1.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-cli-v0.1.0) - 2026-05-04

### Added

- bundle in-flight v1-hardening work + pallet module split

### Other

- apply rustfmt
- initial commit
