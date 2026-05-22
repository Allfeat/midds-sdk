# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-validate-v0.2.0) - 2026-05-22

### Added

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
