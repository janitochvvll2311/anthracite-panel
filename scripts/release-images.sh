#!/bin/sh
# Build Docker-compatible images for the four release variants.
#
# Usage: scripts/release-images.sh --variants <staged-variants> [--out <dir>]
#
# The variants come from scripts/release-variants.sh. The images are assembled
# with umoci and packed with skopeo (docker-archive), so no container runtime or
# namespace is needed to build them; `docker load` or `podman load` consumes the
# tarballs.
#
# Each image carries a minimal Alpine-compatible userspace (static app binaries
# plus busybox), the variant under /opt/anthracite, and a state volume at
# /opt/anthracite/state.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
invoked=$(pwd)
cd "$root"

variants=
out=
while [ $# -gt 0 ]; do
    case "$1" in
        --variants)
            value=${2:?`--variants` needs a directory}
            case "$value" in
                /*) variants=$value ;;
                *) variants=$invoked/$value ;;
            esac
            shift 2
            ;;
        --out)
            value=${2:?`--out` needs a directory}
            case "$value" in
                /*) out=$value ;;
                *) out=$invoked/$value ;;
            esac
            shift 2
            ;;
        -h | --help)
            sed -n '2,14p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done
[ -n "$variants" ] || {
    echo "usage: scripts/release-images.sh --variants <dir> [--out <dir>]" >&2
    exit 2
}
[ -n "$out" ] || out=$variants/images
version=$(sed -n 's/^  "version": "\([^"]*\)",$/\1/p' "$variants/core/manifest.json" | head -1)
[ -n "$version" ] || { echo "no version in $variants/core/manifest.json" >&2; exit 1; }

for tool in umoci skopeo gzip sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "missing $tool; install it (Alpine: apk add umoci skopeo)" >&2
        exit 2
    }
done
[ -f /bin/busybox ] || { echo "this builder needs /bin/busybox" >&2; exit 2; }
[ -f /lib/ld-musl-x86_64.so.1 ] || {
    echo "this builder needs /lib/ld-musl-x86_64.so.1" >&2
    exit 2
}

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/images.XXXXXX)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$out"

# A small Alpine-compatible rootfs: busybox (the run scripts) plus the musl
# loader and a passwd for the non-root variants. The release binaries are static.
userspace() {
    rootfs=$1
    user=$2
    mkdir -p "$rootfs/bin" "$rootfs/lib" "$rootfs/etc" "$rootfs/tmp" \
        "$rootfs/home/anthracite" "$rootfs/opt/anthracite/state"
    cp /bin/busybox "$rootfs/bin/busybox"
    for applet in sh ash cat chmod chown cp cut date dirname echo env grep head id \
        kill ln ls mkdir mktemp mv printf pwd readlink rm sed sleep sort tail tr \
        true false uname wc xargs test '['; do
        ln -sf busybox "$rootfs/bin/$applet"
    done
    cp /lib/ld-musl-x86_64.so.1 "$rootfs/lib/"
    ln -sf ld-musl-x86_64.so.1 "$rootfs/lib/libc.musl-x86_64.so.1"
    printf 'root:x:0:0:root:/root:/bin/sh\n' > "$rootfs/etc/passwd"
    printf 'anthracite:x:1000:1000:Anthracite:/home/anthracite:/bin/sh\n' >> "$rootfs/etc/passwd"
    printf 'root:x:0:\nanthracite:x:1000:\n' > "$rootfs/etc/group"
    printf '127.0.0.1 localhost\n' > "$rootfs/etc/hosts"
    chmod 1777 "$rootfs/tmp"
    chmod 755 "$rootfs/opt/anthracite"
    if [ "$user" = root ]; then
        chown -R 0:0 "$rootfs/opt/anthracite"
        chmod 700 "$rootfs/opt/anthracite/state"
    else
        chown -R 0:0 "$rootfs/opt/anthracite"
        chown 1000:1000 "$rootfs/home/anthracite" "$rootfs/opt/anthracite/state"
        chmod 700 "$rootfs/opt/anthracite/state"
    fi
}

# umoci config takes one value per flag; arrays are repeated flags.
for variant in core base demo demo-sessions; do
    [ -d "$variants/$variant" ] || { echo "no variant $variant in $variants" >&2; exit 1; }
    echo "images: $variant"
    layout=$tmp/$variant-layout
    umoci init --layout "$layout"
    umoci new --image "$layout:$variant"
    umoci unpack --image "$layout:$variant" "$tmp/$variant-root"
    rootfs=$tmp/$variant-root/rootfs

    case "$variant" in
        base)
            user=0:0
            owner=root
            ;;
        *)
            user=1000:1000
            owner=1000:1000
            ;;
    esac
    userspace "$rootfs" "$owner"
    cp -a "$variants/$variant/." "$rootfs/opt/anthracite/"
    chown -R 0:0 "$rootfs/opt/anthracite"
    chmod -R a+rX "$rootfs/opt/anthracite"
    if [ "$owner" != root ]; then
        chown 1000:1000 "$rootfs/opt/anthracite/state"
    fi
    chmod 700 "$rootfs/opt/anthracite/state"
    umoci repack --image "$layout:$variant" "$tmp/$variant-root"

    listen=8090
    ports="--config.exposedports=8090/tcp"
    extra_env="--config.env=HOME=$([ "$owner" = root ] && echo /root || echo /home/anthracite)"
    if [ "$variant" = demo-sessions ]; then
        listen=8091
        ports="--config.exposedports=8091/tcp"
        port=8100
        while [ "$port" -lt 8200 ]; do
            ports="$ports --config.exposedports=$port/tcp"
            port=$((port + 1))
        done
        extra_env="$extra_env --config.env=ANTHRACITE_PORT_BASE=8100 --config.env=ANTHRACITE_PORT_COUNT=100"
    fi
    # shellcheck disable=SC2086
    umoci config --image "$layout:$variant" \
        --architecture amd64 --os linux \
        --config.user "$user" \
        --config.workingdir /opt/anthracite \
        --config.env "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
        $extra_env \
        --config.entrypoint=/opt/anthracite/run.sh \
        --config.entrypoint=--listen \
        --config.entrypoint="0.0.0.0:$listen" \
        --config.volume /opt/anthracite/state \
        --config.stopsignal SIGTERM \
        --config.label "org.opencontainers.image.title=Anthracite $variant" \
        --config.label "org.opencontainers.image.version=$version" \
        --config.label "org.opencontainers.image.description=Anthracite 0.6.0 $variant variant" \
        --config.label "org.opencontainers.image.licenses=Apache-2.0" \
        --config.label "org.opencontainers.image.source=https://github.com/janitochvvll2311/anthracite-panel" \
        $ports

    archive=$out/anthracite-$version-$variant.tar
    rm -f "$archive" "$archive.gz"
    skopeo copy "oci:$layout:$variant" "docker-archive:$archive:anthracite-$variant:$version" >/dev/null
    gzip -9 "$archive"
    echo "images: $out/anthracite-$version-$variant.tar.gz"
done

(
    cd "$out"
    sha256sum anthracite-*.tar.gz > SHA256SUMS
)
echo "images: wrote $out/SHA256SUMS"
