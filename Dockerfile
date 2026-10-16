# syntax=docker/dockerfile:1

# --- Builder -----------------------------------------------------------------
# Alpine ships the musl toolchain, matching the workspace's default target
# (x86_64-unknown-linux-musl in .cargo/config.toml), so the binaries are static.
FROM rust:1.99-alpine AS builder

RUN apk add --no-cache musl-dev gcc
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /src

# The workspace build needs every member's manifest.
COPY Cargo.toml Cargo.lock ./
COPY .cargo .cargo
COPY crates ./crates

RUN cargo build --release --workspace

# --- Runtime -----------------------------------------------------------------
FROM alpine:3.20

# Unprivileged user that owns the configuration directory.
RUN addgroup -S anth \
    && adduser -S -G anth -h /home/anth anth \
    && mkdir -p /etc/anth \
    && chown -R anth:anth /etc/anth \
    && apk add --no-cache curl

# Everything is bundled: daemon, CLI and panel.
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release/anthd /usr/local/bin/anthd
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release/anth-cli /usr/local/bin/anth-cli
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release/anth-panel /usr/local/bin/anth-panel
COPY docker/entrypoint.sh /usr/local/bin/anth-entrypoint
RUN chmod +x /usr/local/bin/anth-entrypoint

ENV ANTH_CONFIG=/etc/anth/config.toml
ENV ANTH_PANEL_ADDR=0.0.0.0:8080
ENV RUST_LOG=info

EXPOSE 8080

USER anth

# Starts the daemon and then the panel; pass a command to run the CLI instead,
# e.g. `docker run --rm <image> anth-cli echo hola`.
ENTRYPOINT ["/usr/local/bin/anth-entrypoint"]
