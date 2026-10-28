#!/usr/bin/env bash
# Build the hosted demo tarball and its container context.
#
#   ./scripts/package-demo.sh            # version 0.3.0a
#   ./scripts/package-demo.sh 0.3.0b
#
# The binary is compiled with the `demo` feature, so it cannot select the real
# host backends. TLS is deliberately absent: a reverse proxy terminates it.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
version=${1:-0.3.0a}
target=x86_64-unknown-linux-musl
name=anthracite-panel-demo-${version}-${target}
dist=$root/dist

if [ -r /tools/env.sh ]; then
  # shellcheck disable=SC1091
  . /tools/env.sh
fi

echo "building the frontend and staging the shell"
(cd "$root/frontend" && bun install --frozen-lockfile && bun run build && ./build-shell.sh)

echo "building the demo daemon"
(cd "$root/backend" && cargo build --release -p anth-panel --features demo --target "$target")
binary=$root/backend/target/$target/release/anth-panel
[ -x "$binary" ] || { echo "missing $binary" >&2; exit 1; }

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
prefix=$staging/$name
mkdir -p "$prefix"/{bin,etc/anth,share/doc/anthracite-panel,share/containers}
cp "$binary" "$prefix/bin/anth-panel-demo"
cp "$root/packaging/demo/config.toml" "$prefix/etc/anth/config.toml"
cp "$root/packaging/demo/Dockerfile" "$prefix/share/containers/Dockerfile"
cp "$root/README.md" "$root/LICENSE" "$prefix/share/doc/anthracite-panel/"
cp -R "$root/docs" "$prefix/share/doc/anthracite-panel/docs"

mkdir -p "$dist"
tar -C "$staging" -czf "$dist/$name.tar.gz" "$name"
(cd "$dist" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "wrote $dist/$name.tar.gz"
echo "wrote $dist/$name.tar.gz.sha256"
ls -lh "$dist/$name.tar.gz"
