# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/v0.2.0) - 2026-05-22

### Added

- *(midds-types)* [**breaking**] make MusicalWork.creation_year optional
- *(midds-types)* [**breaking**] creator carries multiple roles and dual IPI+ISNI identifiers
- *(pallet-midds)* [**breaking**] make `DepositBase` / `DepositPerByte` runtime-mutable
- *(midds-types)* stabilize V1 per-field validation rules
- *(pallet-midds)* paginate identifier lookups, expose count
- *(pallet-midds)* add remove_own_on_behalf with permissive caller
- *(pallet-midds)* [**breaking**] stratify bond into sponsor/owner layers
- bundle in-flight v1-hardening work + pallet module split

### Fixed

- *(pallet-midds)* align mass_injection test with runtime-mutable deposits
- *(pallet-midds)* emit net refund in Refunded event
- *(pallet-midds)* record demand on update grow
- *(pallet-midds)* allow force_edit on already-finalized records
- *(pallet-midds)* mark every dispatchable #[transactional]
- *(pallet-midds)* use checked_add when bumping the on-behalf nonce
- *(pallet-midds)* [**breaking**] domain-separate on-behalf signed payloads
- *(pallet-midds)* [**breaking**] scale multiplier step with deviation amplitude
- *(pallet-midds)* roll over leftover finalizations via cursor

### Other

- drop non-doc inline comments across the workspace
- centralize on-behalf authorization
- *(pallet-midds)* hoist shared remove_own and update settlement helpers
- *(mass-injection)* refresh stale 50k/100k storage-root fixtures
- release v0.1.0
- initial commit

## [0.1.0](https://github.com/Allfeat/midds-sdk/releases/tag/v0.1.0) - 2026-05-04

### Added

- bundle in-flight v1-hardening work + pallet module split

### Other

- initial commit
