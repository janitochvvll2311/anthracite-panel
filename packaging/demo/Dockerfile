# Hosted demo image.
#
# Build from the unpacked demo release directory:
#   docker build -t anthracite-panel-demo:0.3.0a .
#
# The image runs non-root, needs no Linux capabilities and keeps all state in
# /var/lib/anth. Run it with a read-only root and a tmpfs there:
#   docker run --rm --read-only --tmpfs /var/lib/anth:size=256m \
#     -p 127.0.0.1:8080:8080 anthracite-panel-demo:0.3.0a
#
# Put the reverse proxy in front for TLS and rate limiting.
ARG VERSION=0.3.0a

FROM alpine:3.22

ARG VERSION
RUN addgroup -S -g 10001 anth \
    && adduser -S -D -H -u 10001 -G anth anth \
    && mkdir -p /var/lib/anth \
    && chown anth:anth /var/lib/anth

COPY anthracite-panel-demo-${VERSION}-x86_64-unknown-linux-musl/bin/anth-panel-demo /usr/local/bin/anth-panel-demo
COPY anthracite-panel-demo-${VERSION}-x86_64-unknown-linux-musl/etc/anth/config.toml /etc/anth/config.toml

USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/anth-panel-demo"]
CMD ["run", "--config", "/etc/anth/config.toml"]
