# ---- Builder ----
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Build dependencies first so they're cached across source-only changes.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
COPY templates ./templates
RUN touch src/main.rs && cargo build --release

# ---- Runtime ----
FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --shell /usr/sbin/nologin appuser

COPY --from=builder /app/target/release/link-allow-rust ./link-allow-rust
COPY static ./static

USER appuser
ENV PORT=8000
EXPOSE 8000

CMD ["./link-allow-rust"]
