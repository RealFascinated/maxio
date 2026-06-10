# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder

RUN apt-get update \
  && apt-get install -y --no-install-recommends libpq-dev \
  && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://bun.sh/install | bash
ENV PATH="/root/.bun/bin:${PATH}"

# Avoid intermittent crates.io HTTP/2 framing errors in CI/Docker builders.
ENV CARGO_HTTP_MULTIPLEXING=false \
  CARGO_NET_RETRY=10

WORKDIR /app

COPY ui/package.json ui/bun.lock ./ui/
RUN cd ui && bun install --frozen-lockfile

COPY Cargo.toml Cargo.lock build.rs ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/local/cargo/git \
  mkdir src && echo "fn main() {}" > src/main.rs \
  && for i in 1 2 3 4 5; do \
    cargo fetch --locked && break; \
    echo "cargo fetch attempt $i failed, retrying..."; \
    sleep $((i * 5)); \
  done \
  && rm -rf src

COPY src ./src
COPY tests ./tests
COPY ui ./ui

RUN cd ui && bun run build
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/usr/local/cargo/git \
  --mount=type=cache,target=/app/target \
  for i in 1 2 3 4 5; do \
    cargo build --release --locked && break; \
    echo "cargo build attempt $i failed, retrying..."; \
    sleep $((i * 5)); \
  done \
  && cp /app/target/release/maxio /maxio

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates libpq5 \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --create-home --home-dir /nonexistent --shell /usr/sbin/nologin maxio \
  && mkdir -p /data \
  && chown -R maxio:maxio /data

COPY --from=builder /maxio /usr/local/bin/maxio

ENV MAXIO_DATA_DIR="/data"
EXPOSE 9000
VOLUME ["/data"]
USER maxio:maxio
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD ["maxio", "healthcheck", "--url", "http://127.0.0.1:9000/healthz", "--timeout-ms", "2000"]

ENTRYPOINT ["maxio"]
CMD ["serve"]
