#!/bin/sh
# Build every app frontend into its `frontend/dist` module.
#
# The demo bundle builder reads `apps/<app>/frontend/dist/<app>.js`; the
# generated modules are committed so a bundle build does not need Bun. Run this
# after editing any frontend source.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"

bun_bin=${BUN:-bun}
if ! command -v "$bun_bin" >/dev/null 2>&1; then
    if [ -x /tools/bun/bin/bun ]; then
        bun_bin=/tools/bun/bin/bun
    fi
fi
[ -x "$bun_bin" ] || command -v "$bun_bin" >/dev/null 2>&1 || {
    echo "bun was not found; set BUN" >&2
    exit 2
}

for dir in apps/*/frontend; do
    [ -f "$dir/package.json" ] || continue
    echo "== $dir"
    (
        cd "$dir"
        "$bun_bin" install
        "$bun_bin" run typecheck
        if grep -q '"test"' package.json; then
            "$bun_bin" run test
        fi
        "$bun_bin" run build
    )
done
