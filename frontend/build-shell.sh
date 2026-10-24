#!/usr/bin/env bash
# Builds the shell and puts it where the daemon serves it from.
#
# The daemon embeds `backend/anth-panel/www/` at compile time in a release
# build, and reads it from disk in a debug build. Either way this script is what
# fills it, so a released binary and a working tree serve the same bytes.
#
# `harness.html` is deliberately not copied: it is the development page that
# mounts a widget directly, and a deployment has no use for it. The daemon
# excludes it from the lookup as well, so a stale copy cannot be served.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$here/.." && pwd)
dest="$root/backend/anth-panel/www"

echo "building the frontend"
(cd "$root/frontend" && bun install --frozen-lockfile && bun run build)

built="$root/frontend/anth-webui/dist"
[ -f "$built/index.html" ] || { echo "no $built/index.html after the build" >&2; exit 1; }

echo "staging the shell into $dest"
# `www/` is gitignored apart from `.gitkeep`, which is tracked so a fresh clone
# has the directory the daemon expects. Removing the tree with `rm -rf` deleted
# that file too and left the working tree with a staged deletion, so the
# contents are cleared item by item and the placeholder is kept.
find "$dest" -mindepth 1 -maxdepth 1 ! -name .gitkeep -exec rm -rf {} +
mkdir -p "$dest"
# Everything but the harness page. `cp` cannot exclude, so the copy is driven
# by `find` and the exclusion is stated once, next to its reason.
(cd "$built" && find . -type f ! -name harness.html -print0 |
  while IFS= read -r -d '' file; do
    mkdir -p "$dest/$(dirname "$file")"
    cp "$file" "$dest/$file"
  done)

# A build that lost its index would serve 404s that look like a routing bug.
[ -f "$dest/index.html" ] || { echo "the staged shell has no index.html" >&2; exit 1; }
[ ! -f "$dest/harness.html" ] || { echo "harness.html reached the shell" >&2; exit 1; }

echo "wrote $(find "$dest" -type f | wc -l) files into $dest"
