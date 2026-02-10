# Build stage
FROM rust:1.93-alpine AS builder

# Install musl and OpenSSL development headers
RUN apk add musl-dev openssl-dev

WORKDIR /appli/aws/s3_largecopy

# Copy and build dependencies first for better caching
COPY Cargo.toml ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --target x86_64-unknown-linux-musl

# Copy source and build
COPY src/ ./src/
RUN cargo build --release --target x86_64-unknown-linux-musl

# Final stage - minimal image
FROM alpine:latest

# Install ca-certificates for HTTPS
RUN apk --no-cache add ca-certificates

# Copy the binary
COPY --from=builder /appli/aws/s3_largecopy/target/x86_64-unknown-linux-musl/release/s3_largecopy /usr/local/bin/

# Make it executable
RUN chmod +x /usr/local/bin/s3_largecopy

# Run the binary
ENTRYPOINT ["/usr/local/bin/s3_largecopy"]