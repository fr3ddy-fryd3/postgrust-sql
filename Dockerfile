# Multi-stage build for RustDB
# Stage 1: Builder
FROM rust:1-slim-bookworm as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY examples ./examples

# Build release binary
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
  ca-certificates \
  && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 postgres

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/postgrustsql /app/postgrustsql

# Create data directory
RUN mkdir -p /app/data && chown -R postgres:postgres /app

USER postgres

# Expose PostgreSQL default port
EXPOSE 5432

# Run the server
CMD ["./postgrustsql"]
