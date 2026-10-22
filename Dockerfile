# syntax=docker/dockerfile:1

# --- Frontend ----------------------------------------------------------------
# Vite/Lit sources are built with bun; the output feeds the backend's embedded
# www/ folder in the builder stage.
FROM docker.io/oven/bun:1-alpine AS frontend

WORKDIR /src

COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile

COPY frontend ./
RUN bun run build

# --- Builder -----------------------------------------------------------------
# Alpine ships the musl toolchain, matching the workspace's default target
# (x86_64-unknown-linux-musl in backend/.cargo/config.toml), so the binary is static.
FROM docker.io/library/rust:1.99-alpine AS builder

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
FROM docker.io/library/alpine:3.20

# Unprivileged user that owns the configuration directory.
# wireguard-tools/iproute2 make the reference plugin usable when the
# container is granted NET_ADMIN; the host kernel still provides WireGuard.
RUN addgroup -S anth \
    && adduser -S -G anth -h /home/anth anth \
    && mkdir -p /etc/anth \
    && chown -R anth:anth /etc/anth \
    && apk add --no-cache curl nftables libcap wireguard-tools iproute2 \
    && setcap cap_net_admin+ep /usr/sbin/nft
# nftables still needs CAP_NET_ADMIN in the container bounding set:
#   docker run --cap-add NET_ADMIN ...   or compose.yaml

# Account management is opt-in at runtime, and the tools are installed
# unconditionally so the feature is a config change rather than a rebuild.
#
# `shadow` is in `community`, not `main`. BusyBox has adduser/deluser, but no
# usermod, groupmod, chpasswd or gpasswd, and its adduser cannot set a uid, a
# gecos or a shell in one call, so the panel cannot use it. `shadow` pulls
# `linux-pam` for `libpam`, which is also what supplies the `base-password`
# chain that `/etc/pam.d/chpasswd` includes; Alpine's base image has no PAM
# configuration of its own.
#
# The tools need root to write /etc/passwd, /etc/shadow and /etc/sudoers.d.
# The container runs as `anth`, so account management answers PermissionDenied
# unless it is started as root. That is deliberate: see docs/accounts.md.
RUN apk add --no-cache shadow

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
