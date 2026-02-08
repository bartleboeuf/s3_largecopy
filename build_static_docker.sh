#!/bin/bash
# Build script for static binary using Docker

docker run --rm \
  -v "$(pwd)":/appli/aws/s3_largecopy \
  -w /appli/aws/s3_largecopy \
  rust:1.93-alpine \
  sh -c "apk add musl-dev make && rustup target add x86_64-unknown-linux-musl && cargo build --release --target x86_64-unknown-linux-musl"