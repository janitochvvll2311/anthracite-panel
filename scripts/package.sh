#!/usr/bin/env bash
# Build a static Linux tarball with the embedded shell.
#
#   ./scripts/package.sh            # version 0.3.0a
#   ./scripts/package.sh 0.3.0b
#
# The version is a package/tag marker, not a Cargo version: the workspace still
# reports 0.3.0 through `--version`.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
version=${1:-0.3.0a}
target=x86_64-unknown-linux-musl
name=anthracite-panel-${version}-${target}
dist=$root/dist

if [ -r /tools/env.sh ]; then
  # shellcheck disable=SC1091
  . /tools/env.sh
fi

echo "building the frontend and staging the shell"
(cd "$root/frontend" && bun install --frozen-lockfile && bun run build && ./build-shell.sh)

echo "building the release daemon"
(cd "$root/backend" && cargo build --release -p anth-panel --target "$target")
binary=$root/backend/target/$target/release/anth-panel
[ -x "$binary" ] || { echo "missing $binary" >&2; exit 1; }

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
prefix=$staging/$name
mkdir -p "$prefix"/{bin,etc/anth,lib/systemd/system,share/doc/anthracite-panel,share/containers}
cp "$binary" "$prefix/bin/anth-panel"
mkdir -p "$dist"
# `config set` creates a release-default file when the target is missing.
"$binary" --config "$prefix/etc/anth/config.toml.example" config set version 1 >/dev/null
# `config set` takes an advisory lock; it is not part of the release.
rm -f "$prefix/etc/anth/config.toml.example.lock"
cp "$root/packaging/anthracite-panel.service" "$prefix/lib/systemd/system/anthracite-panel.service"
cp "$root/packaging/Dockerfile" "$prefix/share/containers/Dockerfile"
cp "$root/README.md" "$root/LICENSE" "$prefix/share/doc/anthracite-panel/"
cp -R "$root/docs" "$prefix/share/doc/anthracite-panel/docs"
cp -R "$root/plugins" "$prefix/share/doc/anthracite-panel/plugins"
# Keep reference bundles out of the release tree; they are sources, not binaries.
rm -rf "$prefix/share/doc/anthracite-panel/plugins"/*/dist \
       "$prefix/share/doc/anthracite-panel/plugins"/*/backend/target

mkdir -p "$dist"
tar -C "$staging" -czf "$dist/$name.tar.gz" "$name"
(cd "$dist" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "wrote $dist/$name.tar.gz"
echo "wrote $dist/$name.tar.gz.sha256"
ls -lh "$dist/$name.tar.gz"
