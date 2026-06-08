# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-rpc-v0.2.0) - 2026-06-08

### Added

- add Release MIDDS type (UPC/EAN-keyed, third and final V1 type)
- *(midds-rpc)* [**breaking**] per-instance namespaced JSON-RPC via midds_rpc_instance!
- *(pallet-midds)* paginate identifier lookups, expose count
- *(pallet-midds)* [**breaking**] stratify bond into sponsor/owner layers
- bundle in-flight v1-hardening work + pallet module split

### Other

- drop non-doc inline comments across the workspace
- centralize on-behalf authorization
- *(midds-rpc)* expose inherent handlers for multi-instance node wrappers
- release v0.1.0
- initial commit

## [0.1.0](https://github.com/Allfeat/midds-sdk/releases/tag/midds-rpc-v0.1.0) - 2026-05-04

### Added

- bundle in-flight v1-hardening work + pallet module split

### Other

- initial commit
