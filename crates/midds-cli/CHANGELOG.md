# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-cli-v0.2.0) - 2026-05-22

### Added

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
