#!/bin/sh
# The acid test (§1, §11): an app authored outside `apps/` becomes a bundle,
# installs, composes, renders and answers with no engine change.
#
# Usage: scripts/acid-test.sh
#
# The app lives in tests/acid/app, outside the apps workspace; it is built with
# plain cargo, packaged and signed with the tools a third party would use, and
# run against a real panel. ACID_PORT picks the port (default 8098).
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
cd "$root"
port=${ACID_PORT:-8098}
mkdir -p /tmp/anthracite-panel
dir=$(mktemp -d /tmp/anthracite-panel/acid.XXXXXX)
panel=
trap 'if [ -n "$panel" ]; then kill "$panel" 2>/dev/null || true; fi; rm -rf "$dir"' EXIT

echo "acid: building the tools"
cargo build --manifest-path sdk/rust/Cargo.toml -p anth-cli -p anth-signer -p anth-conformance
cargo build --manifest-path panel/backend/Cargo.toml

echo "acid: building the external component"
cargo build --manifest-path tests/acid/app/Cargo.toml --release --target wasm32-unknown-unknown
module=target/wasm32-unknown-unknown/release/acid_ping.wasm
[ -f "$module" ] || { echo "cargo did not produce $module" >&2; exit 2; }

wasm_tools=
for candidate in "${WASM_TOOLS:-}" wasm-tools "${CARGO_HOME:-}/bin/wasm-tools" /tools/cargo/bin/wasm-tools; do
    [ -n "$candidate" ] || continue
    if command -v "$candidate" >/dev/null 2>&1; then
        candidate=$(command -v "$candidate")
    fi
    if [ -x "$candidate" ]; then
        wasm_tools=$candidate
        break
    fi
done
[ -n "$wasm_tools" ] || { echo "wasm-tools not found; set WASM_TOOLS" >&2; exit 2; }
"$wasm_tools" component embed --world app tests/acid/app/wit "$module" -o "$dir/embedded.wasm"
"$wasm_tools" component new "$dir/embedded.wasm" -o "$dir/component.wasm"

echo "acid: laying out and signing the bundle"
app=$dir/app
mkdir -p "$app/wasm" "$app/wit" "$app/web"
mv "$dir/component.wasm" "$app/wasm/app.wasm"
cp tests/acid/app/wit/app.wit "$app/wit/"
cp tests/acid/app/web/ping.js "$app/web/"
sha=$(sha256sum "$app/wasm/app.wasm" | cut -d' ' -f1)
module_sha=$(sha256sum "$app/web/ping.js" | cut -d' ' -f1)
sed -e "s/{{sha256}}/$sha/" -e "s/{{module_sha256}}/$module_sha/" \
    tests/acid/app/manifest.toml.in > "$app/manifest.toml"

generated=$(./target/debug/anth-sign keygen --out "$dir/key")
key=$(printf '%s\n' "$generated" | awk '/^public_key/ {print $2}')
test -n "$key"
ANTHRACITE_SIGNING_KEY="$dir/key" ANTH_SIGN=./target/debug/anth-sign \
    scripts/package-app.sh "$app" "$dir/ping.zip"
./target/debug/anth-conformance --require-signature --key "$key" "$dir/ping.zip"

echo "acid: starting the panel"
cat > "$dir/config.toml" <<EOF
version = 1
socket = "$dir/engine.sock"
state = "$dir/state"

[apps]
require-signature = true
EOF

start_panel() {
    ./target/debug/anth-panel --listen "127.0.0.1:$port" \
        --config "$dir/config.toml" >> "$dir/panel.log" 2>&1 &
    panel=$!
    for _ in $(seq 1 100); do
        if curl -sf "http://127.0.0.1:$port/health" >/dev/null; then
            return 0
        fi
        sleep 0.1
    done
    cat "$dir/panel.log" >&2
    return 1
}

anth="./target/debug/anth --socket $dir/engine.sock"
start_panel
$anth key add "$key" --label acid-test
$anth app install "$dir/ping.zip"
$anth app approve org.example.ping
$anth compose --add org.example.ping
kill "$panel"
wait "$panel" 2>/dev/null || true
panel=

echo "acid: the external app answers over the CLI"
start_panel
answer=$($anth app ping now)
case "$answer" in
    *pong*) ;;
    *) echo "the app did not answer: $answer" >&2; exit 1 ;;
esac

echo "acid: the panel serves the module and reports the slot"
password=$(sed -n 's/.*"password":"\([^"]*\)".*/\1/p' "$dir/state/credentials.txt")
new_password=acid-test-password-123
curl -sf -c "$dir/cookies" -H 'content-type: application/json' \
    -d "{\"operator\":\"operator\",\"password\":\"$password\"}" \
    "http://127.0.0.1:$port/api/engine/session" >/dev/null
curl -sf -b "$dir/cookies" -c "$dir/cookies" -X PATCH \
    -H 'content-type: application/json' \
    -d "{\"current_password\":\"$password\",\"new_password\":\"$new_password\"}" \
    "http://127.0.0.1:$port/api/engine/operator" >/dev/null

body=$(curl -sf -b "$dir/cookies" \
    "http://127.0.0.1:$port/api/apps/org.example.ping/ping")
case "$body" in
    *pong*) ;;
    *) echo "the route did not answer: $body" >&2; exit 1 ;;
esac
curl -sf -b "$dir/cookies" -D "$dir/assets.headers" -o "$dir/ping.js" \
    "http://127.0.0.1:$port/api/apps/org.example.ping/assets/web/ping.js"
cmp "$dir/ping.js" tests/acid/app/web/ping.js
grep -qi 'cache-control:.*immutable' "$dir/assets.headers"

surface=$(curl -sf -b "$dir/cookies" "http://127.0.0.1:$port/api/engine/composition")
printf '%s' "$surface" | grep -q 'acid-ping'
printf '%s' "$surface" | grep -q 'org.example.ping'

echo "acid test passed: org.example.ping installed, composed, served and answered"
