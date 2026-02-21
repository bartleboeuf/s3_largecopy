# Installation Guide

This document describes how to set up `s3_largecopy` on your system.

## Prerequisites

- **Rust 1.93.0** or later
- **AWS credentials** configured (via `aws configure`, environment variables, or SSO)
- Sufficient IAM permissions on source and destination buckets

## Downloading Pre-built Binaries (Fastest)

If you don't want to build from source, you can download pre-compiled, statically linked binaries from the [GitHub Releases](https://github.com/bartleboeuf/s3_largecopy/releases) page. We provide assets for multiple platforms:

- **Linux (x86_64)**: Standard and Statically linked (musl)
- **Linux (ARM64/aarch64)**: Standard and Statically linked (musl)
- **macOS**: Apple Silicon (aarch64) and Intel (x86_64)
- **Windows**: x86_64 executable

### Installation Steps

1.  **Download the archive** for your platform from the Releases page.
2.  **Extract the binary**:
    - **Linux/macOS**: `tar -xzf s3_largecopy-v1.x.x-macos-aarch64.tar.gz`
    - **Windows**: Extract the `.zip` file using Explorer or PowerShell.
3.  **Make it executable** (Linux/macOS):
    ```bash
    chmod +x s3_largecopy
    ```
4.  **Install it** (optional, to run from anywhere):
    - **Linux/macOS**: `sudo mv s3_largecopy /usr/local/bin/`
    - **Windows**: Add the folder containing `s3_largecopy.exe` to your `PATH` environment variable.

### Quick Command (Linux x86_64 Static)
```bash
RELEASE_VERSION="v1.0.0" # Replace with actual version
curl -L -O https://github.com/bartleboeuf/s3_largecopy/releases/download/${RELEASE_VERSION}/s3_largecopy-${RELEASE_VERSION}-linux-x86_64-static.tar.gz
tar -xzf s3_largecopy-${RELEASE_VERSION}-linux-x86_64-static.tar.gz
chmod +x s3_largecopy
```

## Building from Source

### Standard Build
Recommended if you are running the tool on the same machine where you build it.

```bash
# Clone the repository
git clone https://github.com/bartleboeuf/s3_largecopy
cd s3_largecopy

# Build the release binary
cargo build --release

# The binary will be at target/release/s3_largecopy
```

### Static Binary Build (Recommended for Portability)
Static binaries include all necessary libraries (via `musl-libc`) and work on any Linux server regardless of the local GLIBC version.

**Using Docker:**
```bash
# Build static binary using Docker (Alpine-based Rust image)
docker run --rm \
  -v "$(pwd)":/appli/aws/s3_largecopy \
  -w /appli/aws/s3_largecopy \
  rust:1.93-alpine \
  sh -c "apk add musl-dev perl make && \
         rustup target add x86_64-unknown-linux-musl && \
         cargo build --release --target x86_64-unknown-linux-musl"
```

**Using the build script:**
```bash
chmod +x build_static_docker.sh
./build_static_docker.sh
```

The resulting binary will be located at: `target/x86_64-unknown-linux-musl/release/s3_largecopy`.

**Verification:**
```bash
file target/x86_64-unknown-linux-musl/release/s3_largecopy
# Output should include: "statically linked"
```
