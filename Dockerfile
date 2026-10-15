# =========================================================================
# Этап 1: Builder
# =========================================================================
FROM rust:slim AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libproj-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

ARG SQLX_OFFLINE=true
ENV SQLX_OFFLINE=${SQLX_OFFLINE}
ENV PROJ_DIR=/usr

COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml ./core/Cargo.toml

RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && mkdir -p core/src && touch core/src/lib.rs \
    && cargo build --release \
    && rm -rf src core/src target/release/deps/geo_storage_service* target/release/deps/libgeo_core*

COPY src ./src
COPY core ./core
COPY .sqlx ./.sqlx

RUN cargo build --release

# =========================================================================
# Этап 2: Image
# =========================================================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    libproj25 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

ENV RUST_LOG=info
ENV PROJ_DATA=/usr/share/proj

COPY --from=builder /app/target/release/geo_storage_service .
COPY migrations ./migrations

EXPOSE 3000

CMD ["./geo_storage_service"]
