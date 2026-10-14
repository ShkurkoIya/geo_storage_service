# =========================================================================
# Этап 1: Builder
# =========================================================================
FROM rust:1.79-slim AS builder

# 📐 Настраиваем системные либы для ГИС-движка PROJ и линковщика C++
RUN apt-get update && apt-get install -y \
    pkg-config \
    libproj-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 🛡️ ПРОБРОС ПЕРЕМЕННЫХ СБОРКИ (BUILD ARGUMENTS)
# Говорим sqlx использовать сохраненный кэш типов во время сборки контейнера
ARG SQLX_OFFLINE=true
ENV SQLX_OFFLINE=${SQLX_OFFLINE}

# Прокидываем пути для системного движка проекций PROJ
ENV PROJ_DIR=/usr

# Копируем манифесты зависимостей Cargo
COPY Cargo.toml Cargo.lock ./
COPY core/Cargo.toml ./core/Cargo.toml

# 🔥 Элитарный кэш-хак Red Group:
# Собираем пустые заглушки, чтобы намертво закешировать скачанные либы (tokio, axum, geozero)
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && mkdir -p core/src && touch core/src/lib.rs \
    && cargo build --release \
    && rm -rf src core/src target/release/deps/geo_storage_service* target/release/deps/libgeo_core*

# Копируем честные маркшейдерские исходники и кэш sqlx (если он сгенерирован)
COPY src ./src
COPY core ./core
COPY .sqlx ./.sqlx

# Финальный компиляционный штурм бинарника
RUN cargo build --release

# =========================================================================
# Этап 2: Production Image
# =========================================================================
FROM debian:bookworm-slim

# Устанавливаем рантайм-библиотеку PROJ для пересчета Гаусса-Крюгера
RUN apt-get update && apt-get install -y \
    libproj25 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Настраиваем переменные окружения для рантайма ГИС внутри контейнера
ENV RUST_LOG=info
ENV PROJ_DATA=/usr/share/proj

# Забираем скомпилированный бинарник из билдера
COPY --from=builder /app/target/release/geo_storage_service .

# Копируем папку с нашими SQL-миграциями, чтобы DbManager мог накатить их прямо при старте контейнера!
COPY migrations ./migrations

EXPOSE 3000

CMD ["./geo_storage_service"]
