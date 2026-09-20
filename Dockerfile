# ── 1: SvelteKit static build ────────────────────────────────────
FROM node:24-alpine AS web
# path prefix baked into the SPA (e.g. /sideout); empty = served at /
ARG SO_BASE=""
ENV SO_BASE=$SO_BASE
WORKDIR /app/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ .
RUN npm run build

# ── 2: Rust release build ────────────────────────────────────────
# suite-pinned: builder and runtime must name the SAME Debian suite,
# otherwise the binary can stop loading in the runtime image.
FROM rust:1.96-slim-bookworm AS rust
WORKDIR /app/server
COPY server/ .
RUN cargo build --release

# ── 3: slim runtime ──────────────────────────────────────────────
FROM debian:bookworm-slim
ARG SO_BASE=""
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust /app/server/target/release/sideout-server /app/server
COPY --from=web /app/web/build /app/web/build
ENV SO_DATA=/data \
    SO_STATIC=/app/web/build \
    SO_ADDR=0.0.0.0:8080 \
    SO_BASE=$SO_BASE
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["/app/server"]
