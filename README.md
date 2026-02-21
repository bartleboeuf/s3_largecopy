# S3 Large File Copy Tool

[![Release](https://github.com/bartleboeuf/s3_largecopy/actions/workflows/release.yml/badge.svg)](https://github.com/bartleboeuf/s3_largecopy/actions/workflows/release.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A high-performance Rust CLI for moving large files (>5GB) between Amazon S3 buckets. Built for speed, cost-efficiency, and reliability.

## Why this tool?

I wrote this application due to a lack of tools and simplicity for moving large files between S3 buckets. Currently, AWS's simple copy object does not support objects larger than 5 GB in a single operation ([see official documentation](https://docs.aws.amazon.com/AmazonS3/latest/API/API_CopyObject.html)). Another solution available from AWS Support is to use a Python Lambda with S3 batch operations. Unfortunately, it takes hours and often fails after the 15-minute timeout for large files.
My goal was simple: I wanted a clean and simple command-line interface (CLI) that could do the job (**VERY**) fast, clean, and with a lot of tuning, options, and customization.
It is satisfying to see that the transfer of a single 100 GB file takes less than 30 seconds with a simple command line, from an EC2 instance in my VPC. 

## Key Features

- **🚀 Performance**: High-concurrency multipart engine with adaptive tuning.
- **💰 Cost Aware**: Real-time cost estimation and request optimization.
- **🛠️ Flexible**: Support for all storage classes, KMS encryption, and property preservation.
- **✅ Reliable**: Automatic cleanup on failure and checksum-based integrity verification.
- **🚄 Auto-Mode**: Intelligent optimization of part sizes and thread counts.

## Quick Start

```bash
# Basic copy
./s3_largecopy -s source-bucket -k data.iso -b dest-bucket -t data.iso
```

## Documentation

Detailed documentation is available in the `docs/` directory:

- [**Installation**](./docs/INSTALLATION.md) - Prerequisites and build instructions (incl. static binaries).
- [**Usage & Examples**](./docs/USAGE.md) - Basic commands, advanced features, and CLI reference.
- [**Architecture**](./docs/ARCHITECTURE.md) - How the engine works, diagrams, and internal modules.
- [**Auto Mode**](./docs/AUTO_MODE.md) - Deep dive into adaptive tuning and profiles.
- [**Cost Analysis**](./docs/COST_ANALYSIS.md) - Comprehensive guide to S3 transfer costs and estimation logic.
- [**Permissions**](./docs/PERMISSIONS.md) - IAM policy requirements and security configuration.
- [**Troubleshooting**](./docs/TROUBLESHOOTING.md) - Performance tips and common error resolutions.
- [**Changelog**](./CHANGELOG.md) - Detailed history of changes and releases.

## Author

Created and maintained by **[Bart Leboeuf](https://github.com/bartleboeuf)**.

## License

This project is licensed under the [MIT License](LICENSE).

## Contributing

Contributions are welcome! See the [Future Features](FUTURE_FEATURES.md) for roadmap ideas.
