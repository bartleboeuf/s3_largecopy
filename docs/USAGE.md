# Usage Guide

This document provides detailed usage examples and a complete reference for command-line options.

## Basic Usage

The most common use case is copying a single large file between two buckets in the same region:

```bash
./s3_largecopy \
    -s my-source-bucket \
    -k path/to/large-file.iso \
    -b my-dest-bucket \
    -t path/to/copy/large-file.iso
```

## Advanced Examples

### Auto-Tuning Mode
Let the tool automatically optimize part size and concurrency based on file size and network throughput.

```bash
./s3_largecopy -s src -k file -b dst -t file --auto
```

### Cost-Efficient Transfer
Prioritize lower API request counts (larger parts) to save money on multipart overhead.

```bash
./s3_largecopy -s src -k file -b dst -t file --auto --auto-profile cost-efficient
```

### Cross-Region Copy
Copy between buckets in different AWS regions.

```bash
./s3_largecopy -s src -k file -b dst -t file -r us-east-1 --dest-region eu-west-1
```

### Changing Storage Class
Move data to a different storage class (e.g., `INTELLIGENT_TIERING`, `GLACIER_IR`).

```bash
./s3_largecopy -s src -k file -b dst -t file --storage-class DEEP_ARCHIVE
```

### Data Integrity (Checksums)
Enable additional checksum validation (CRC32, CRC32C, SHA1, or SHA256) during transfer.

```bash
./s3_largecopy -s src -k file -b dst -t file --checksum-algorithm SHA256
```

### Cost Estimation
Get a precise cost breakdown before running the actual copy.

```bash
./s3_largecopy -s src -k file -b dst -t file --estimate
```

## Command Line Reference

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--source-bucket` | `-s` | Source S3 bucket name | Required |
| `--source-key` | `-k` | Source object key | Required |
| `--dest-bucket` | `-b` | Destination S3 bucket name | Required |
| `--dest-key` | `-t` | Destination object key | Required |
| `--region` | `-r` | AWS region (for source and default dest) | Default provider |
| `--dest-region` | | Destination region (for cross-region) | Same as `--region` |
| `--part-size` | `-p` | Part size in MB (5-5120) | 256 |
| `--concurrency` |  | Number of concurrent uploads (1-1000) | 50 |
| `--storage-class` |  | Target storage class | Source/default |
| `--auto` | | Enable automatic transfer tuning | `false` |
| `--auto-profile` | | Tuning profile (`balanced`, `aggressive`, `cost-efficient`) | `balanced` |
| `--dry-run` | | Simulate copy without modifying data | `false` |
| `--estimate` | | Print cost estimate and exit | `false` |
| `--force-copy` | | Always overwrite destination | `false` |
| `--verify-integrity` | | Verification mode (`off`, `etag`, `checksum`) | `etag` |
| `--checksum-algorithm` | | Checksum algorithm (CRC32, SHA256, etc.) | None |
| `--sse` | | Encryption algorithm (AES256, aws:kms) | None |
| `--sse-kms-key-id` | | KMS Key ID for aws:kms | None |
| `--no-metadata` | | Disable replication of metadata headers | `false` |
| `--no-tags` | | Disable replication of S3 object tags | `false` |
| `--quiet` | `-q` | Suppress informational output | `false` |
