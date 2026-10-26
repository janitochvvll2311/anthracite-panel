#!/usr/bin/env bash
# Builds the panel and stages it where the daemon serves it from.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$here/.." && pwd)
dest="$root/backend/anth-panel/www"

echo "building the frontend"
(cd "$root/frontend" && bun install --frozen-lockfile && bun run build)

built="$root/frontend/dist"
[ -f "$built/index.html" ] || { echo "no $built/index.html after the build" >&2; exit 1; }

echo "staging the shell into $dest"
find "$dest" -mindepth 1 -maxdepth 1 ! -name .gitkeep -exec rm -rf {} +
mkdir -p "$dest"
(cd "$built" && find . -type f -print0 |
  while IFS= read -r -d '' file; do
    mkdir -p "$dest/$(dirname "$file")"
    cp "$file" "$dest/$file"
  done)

[ -f "$dest/index.html" ] || { echo "the staged shell has no index.html" >&2; exit 1; }
echo "wrote $(find "$dest" -type f | wc -l) files into $dest"
