# Stage 1: Build
FROM rust:latest AS builder

WORKDIR /app

# Install minimal build deps (for sqlx, reqwest TLS, etc.)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates build-essential \
    && rm -rf /var/lib/apt/lists/*

# Pre-build deps (cache)
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release || true

# Build real source
COPY . .
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin wood-engine

# Stage 2: Runtime
FROM debian:stable-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/wood-engine /usr/local/bin/wood-engine

ENV RUST_LOG=info \
    PORT=8000

EXPOSE 8000

CMD ["rev-engine"]