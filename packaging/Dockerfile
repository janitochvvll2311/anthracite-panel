# Build from the unpacked release directory:
#   docker build -t anthracite-panel:0.3.0a .
#
# The daemon needs CAP_NET_ADMIN and CAP_SYS_ADMIN for the network, firewall and
# storage slices; run it with --cap-add for those workflows.
ARG VERSION=0.3.0a

FROM alpine:3.22

ARG VERSION
RUN apk add --no-cache \
    ca-certificates \
    iproute2 \
    nftables \
    util-linux \
    shadow \
    wireguard-tools

COPY anthracite-panel-${VERSION}-x86_64-unknown-linux-musl/bin/anth-panel /usr/local/bin/anth-panel
COPY anthracite-panel-${VERSION}-x86_64-unknown-linux-musl/etc/anth/config.toml.example /etc/anth/config.toml

EXPOSE 8080
VOLUME ["/etc/anth", "/var/lib/anth"]
ENTRYPOINT ["/usr/local/bin/anth-panel"]
CMD ["run", "--config", "/etc/anth/config.toml"]
