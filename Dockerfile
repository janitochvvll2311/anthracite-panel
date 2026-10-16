# syntax=docker/dockerfile:1

# --- Frontend ----------------------------------------------------------------
# Vite/Lit sources are built with bun; the output feeds the backend's embedded
# www/ folder in the builder stage.
FROM oven/bun:1-alpine AS frontend

WORKDIR /src

COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend ./
RUN bun run build

# --- Builder -----------------------------------------------------------------
# Alpine ships the musl toolchain, matching the workspace's default target
# (x86_64-unknown-linux-musl in backend/.cargo/config.toml), so the binary is static.
FROM rust:1.99-alpine AS builder

RUN apk add --no-cache musl-dev gcc
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /src/backend

# The workspace build needs every member's manifest.
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/.cargo .cargo
COPY backend/anth-proto ./anth-proto
COPY backend/anth-panel ./anth-panel

# rust-embed bakes www/ into the binary, so it must be present before compiling.
COPY --from=frontend /src/dist ./anth-panel/www

RUN cargo build --release --workspace

# --- Runtime -----------------------------------------------------------------
FROM alpine:3.20

# Unprivileged user that owns the configuration directory.
RUN addgroup -S anth \
    && adduser -S -G anth -h /home/anth anth \
    && mkdir -p /etc/anth \
    && chown -R anth:anth /etc/anth \
    && apk add --no-cache curl

# Daemon, CLI and HTTP API are a single binary.
COPY --from=builder /src/backend/target/x86_64-unknown-linux-musl/release/anth-panel /usr/local/bin/anth-panel
COPY docker/entrypoint.sh /usr/local/bin/anth-entrypoint
RUN chmod +x /usr/local/bin/anth-entrypoint

ENV ANTH_CONFIG=/etc/anth/config.toml
ENV ANTH_PANEL_ADDR=0.0.0.0:8080
ENV RUST_LOG=info

EXPOSE 8080

USER anth

# Starts the daemon and the API in one process; pass a command to run the CLI
# instead, e.g. `docker run --rm <image> anth-panel echo hola`.
ENTRYPOINT ["/usr/local/bin/anth-entrypoint"]
