#!/bin/sh
# Builds the guest and writes a bundle with the real digest in its manifest.
#
# The checked-in manifest carries a zero `sha256` so a stale one can never look
# like a valid install. This script is where the real value is substituted, and
# it happens on a copy so the repository stays build-independent.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out="$here/dist"
wasm="$here/target/wasm32-unknown-unknown/release/anth_plugin_wireguard.wasm"

cargo build --manifest-path "$here/Cargo.toml" --target wasm32-unknown-unknown --release

digest=$(sha256sum "$wasm" | cut -d' ' -f1)
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

mkdir -p "$stage/web" "$out"
sed "s/0000000000000000000000000000000000000000000000000000000000000000/$digest/" \
  "$here/plugin.toml" > "$stage/plugin.toml"
cp "$wasm" "$stage/plugin.wasm"
cp "$here"/web/*.js "$stage/web/"

# `zip` is used only when it exists; the Python fallback keeps the script usable
# on a host where the panel is staged from a minimal image.
if command -v zip >/dev/null 2>&1; then
  (cd "$stage" && zip -qr "$out/org.anth.wireguard.zip" plugin.toml plugin.wasm web)
else
  python3 - "$stage" "$out/org.anth.wireguard.zip" <<'PY'
import pathlib, sys, zipfile
stage, target = pathlib.Path(sys.argv[1]), sys.argv[2]
with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(stage.rglob("*")):
        if path.is_file():
            archive.write(path, path.relative_to(stage).as_posix())
PY
fi

echo "wrote $out/org.anth.wireguard.zip"
echo "sha256 $digest"
