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
#
# Signing is optional here and required by the host unless
# `[plugins] require_signature = false`. `ANTH_PLUGIN_SIGNING_KEY` names an
# Ed25519 private key in PEM form; the host stores only the matching public key
# in `[plugins] public_key`. See README.md for the two commands that produce it.
if [ -n "${ANTH_PLUGIN_SIGNING_KEY:-}" ]; then
  command -v openssl >/dev/null 2>&1 || {
    echo "error: ANTH_PLUGIN_SIGNING_KEY is set but openssl is not installed" >&2
    exit 1
  }
  openssl pkeyutl -sign -rawin -inkey "$ANTH_PLUGIN_SIGNING_KEY" -in "$wasm" > "$stage/plugin.sig.raw"
  od -An -v -tx1 "$stage/plugin.sig.raw" | tr -d ' \n' > "$stage/plugin.sig"
  rm -f "$stage/plugin.sig.raw"
  echo "signed with $ANTH_PLUGIN_SIGNING_KEY"
else
  echo "warning: bundle is unsigned; the host needs [plugins] require_signature = false to install it" >&2
fi

if command -v zip >/dev/null 2>&1; then
  if [ -f "$stage/plugin.sig" ]; then
    (cd "$stage" && zip -qr "$out/org.anth.wireguard.zip" plugin.toml plugin.wasm plugin.sig web)
  else
    (cd "$stage" && zip -qr "$out/org.anth.wireguard.zip" plugin.toml plugin.wasm web)
  fi
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
