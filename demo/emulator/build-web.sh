#!/bin/sh
# Rebuild the Files module with its bundled ProseMirror dependency.
#
# The iframe forbids remote imports, so the app serves one self-contained ES
# module. Run this after editing `web-src/fs.js`; the generated `web/fs.js` is
# committed so the bundle build itself does not need Bun.
set -eu
cd "$(dirname "$0")"
bun install
bun build bundles/fs/web-src/fs.js \
    --outfile bundles/fs/web/fs.js \
    --format esm \
    --target browser
