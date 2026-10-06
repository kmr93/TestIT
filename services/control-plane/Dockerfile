# Multi-stage Dockerfile for TestIT Rust Control Plane
FROM rust:slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace and manifests
COPY Cargo.toml Cargo.lock* ./
COPY services/control-plane/Cargo.toml ./services/control-plane/
COPY apps/cli/Cargo.toml ./apps/cli/

# Copy source code and migrations
COPY migrations ./migrations
COPY schemas ./schemas
COPY services/control-plane ./services/control-plane
COPY apps/cli ./apps/cli

# Build release binary
RUN cargo build --release --bin testit-control-plane

# Final runtime image
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Create data directories
RUN mkdir -p /data/db /data/artifacts

# Copy compiled binary from builder
COPY --from=builder /app/target/release/testit-control-plane /usr/local/bin/testit-control-plane

ENV PORT=8080
ENV DATABASE_URL="sqlite:///data/db/testit.sqlite?mode=rwc"
ENV ARTIFACTS_DIR="/data/artifacts"

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health/ready || exit 1

ENTRYPOINT ["/usr/local/bin/testit-control-plane"]
