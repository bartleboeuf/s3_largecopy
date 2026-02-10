# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/bartleboeuf/s3_largecopy/compare/v1.0.4...HEAD
[1.0.4]: https://github.com/bartleboeuf/s3_largecopy/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/bartleboeuf/s3_largecopy/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/bartleboeuf/s3_largecopy/compare/774a656...v1.0.2
