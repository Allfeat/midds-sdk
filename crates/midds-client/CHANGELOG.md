# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-client-v0.2.0) - 2026-05-22

### Added

- add Release MIDDS type (UPC/EAN-keyed, third and final V1 type)
- *(midds-client)* add recordings() pallet-instance accessor
- *(pallet-midds)* [**breaking**] stratify bond into sponsor/owner layers
- bundle in-flight v1-hardening work + pallet module split

### Fixed

- *(midds-client)* [**breaking**] read at the best block instead of the finalised one
- *(midds-client)* [**breaking**] reconcile RUNTIME_API_NAME with per-kind runtime traits

### Other

- drop non-doc inline comments across the workspace
- release v0.1.0
- add GitHub Actions workflow
- apply rustfmt
- initial commit

## [0.1.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-client-v0.1.0) - 2026-05-04

### Added

- bundle in-flight v1-hardening work + pallet module split

### Other

- add GitHub Actions workflow
- apply rustfmt
- initial commit
