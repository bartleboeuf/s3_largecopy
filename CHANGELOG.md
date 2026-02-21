# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.7] - 2026-02-21

### Added
- **Live Pricing Tool**: New `--get-price` flag to fetch real-time S3 pricing for any region and storage class.
- **Dynamic Cost Estimation**: The `--estimate` command now integrates with the **AWS Price List API** to provide real-time cost projections based on current S3 rates.
- **Architectural Diagram**: Added an "Internal Architecture" Mermaid diagram to `README.md` to visualize module boundaries.

### Changed
- **Modular Refactoring**: Decoupled `main.rs` into specialized modules: `estimate.rs` (orchestration), `s3_utils.rs` (region detection), and `pricing.rs` (pricing logic).
- **Service Layer**: Introduced a cleaner service layer architecture for S3 utilities and pricing data.

## [1.0.6] - 2026-02-14

### Added
- **Auto-Tuning Mode**: New `--auto` flag to dynamically select transfer parameters based on object size, region, and profile.
- **Auto Profiles**: New `--auto-profile` flag to choose from predefined tuning profiles (`balanced`, `aggressive`, `conservative`, `cost-efficient`).
- **Auto-Tuning Documentation**: New `docs/AUTO_MODE.md` explaining auto-tuning behavior and profiles.

### Changed
- **Auto-Tuning Implementation**: Added `auto.rs` module for auto-tuning logic.

## [1.0.5] - 2026-02-10

### Added
- **Cost Estimation**: New `--estimate` flag to calculate API requests, data transfer, and storage costs before execution. 
- **Encryption Support**: New `--sse` and `--sse-kms-key-id` flags for Server-Side Encryption (SSE-S3 and SSE-KMS).
- **Dry Run Mode**: New `--dry-run` flag to simulate transfers without moving data.
- **Checksum Validation**: New `--checksum-algorithm` flag to verify data integrity using CRC32, CRC32C, SHA1, or SHA256.
- **Cross-Region Estimation**: Support for calculating costs across different AWS regions with `--dest-region`.
- **Public API**: Added `get_source_size()` to `S3CopyApp` for external cost estimation.

### Changed
- **Auto-Tuning**: Enhanced auto-tuning logic to better handle small files (<5GB) via Instant Copy.
- **Architecture**: Refactored `main.rs` to separate estimation flow from execution flow.
- **Documentation**: Updated README with new features, architecture diagrams, and cost estimation examples.

## [1.0.4] - 2026-02-08

### Fixed
- Update release action workflow configuration.

## [1.0.3] - 2026-02-08

### Changed
- Update action release template.

## [1.0.2] - 2026-02-08

### Changed
- Update for github action release workflow.
