#!/bin/sh
# Build the distribution variants from a signed release:
#
#   core  the panel, CLI and engine with an empty composition
#   base  the same binaries plus the signed system-app bundles
#   demo  base replaced by the host-emulator bundles; every run gets its own
#         persistent state directory
#
# Usage: scripts/release-variants.sh --release <signed-release-dir> [--out <dir>]
#
# The signed release comes from scripts/release.sh. Each variant keeps its own
# README, manifest, SHA256SUMS and a run.sh that needs nothing outside it.
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
invoked=$(pwd)
cd "$root"

release=
out=
while [ $# -gt 0 ]; do
    case "$1" in
        --release)
            value=${2:?`--release` needs a directory}
            case "$value" in
                /*) release=$value ;;
                *) release=$invoked/$value ;;
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
            sed -n '2,13p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done
[ -n "$release" ] || {
    echo "usage: scripts/release-variants.sh --release <signed-release-dir> [--out <dir>]" >&2
    exit 2
}
[ -f "$release/manifest.json" ] || { echo "no manifest.json in $release" >&2; exit 2; }
[ -x "$release/bin/anth-panel" ] || { echo "no panel binary in $release/bin" >&2; exit 2; }
[ -n "$out" ] || out=$release/variants

version=$(sed -n 's/^  "version": "\([^"]*\)",$/\1/p' "$release/manifest.json" | head -1)
public=$(sed -n 's/.*"public_key": "\([^"]*\)".*/\1/p' "$release/manifest.json" | head -1)
fingerprint=$(sed -n 's/.*"fingerprint": "\([^"]*\)".*/\1/p' "$release/manifest.json" | head -1)
[ -n "$version" ] && [ -n "$public" ] && [ -n "$fingerprint" ] || {
    echo "cannot read version, public key or fingerprint from $release/manifest.json" >&2
    exit 1
}

system_apps="accounts firewall fs home net proc storage terminal"
arch=$(uname -m)
platform="linux-$arch"

mkdir -p /tmp/anthracite-panel
tmp=$(mktemp -d /tmp/anthracite-panel/variants.XXXXXX)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$out"
rm -rf "$out/core" "$out/base" "$out/demo" "$out/demo-sessions"

# The runner every variant ships. It reads variant.conf, prepares a state
# directory, starts the panel, installs and composes the bundles when there are
# any, and prints where the credentials and the state live.
run_script() {
    cat > "$1/run.sh" <<'RUN'
#!/bin/sh
# Starts this Anthracite variant. State lives in a directory of its own; a demo
# session keeps it unless --ephemeral is passed.
set -eu
here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
. "$here/variant.conf"

listen=${ANTHRACITE_LISTEN:-127.0.0.1:8090}
state=
ephemeral=no
while [ $# -gt 0 ]; do
    case "$1" in
        --listen)
            listen=${2:?`--listen` needs an address}
            shift 2
            ;;
        --state)
            state=${2:?`--state` needs a directory}
            shift 2
            ;;
        --ephemeral)
            ephemeral=yes
            shift
            ;;
        -h | --help)
            echo "usage: run.sh [--listen <addr>] [--state <dir>] [--ephemeral]"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done
[ -n "$listen" ] || { echo "an empty --listen is not usable" >&2; exit 2; }

if [ -z "$state" ]; then
    if [ "$SESSION_STATE" = yes ]; then
        state=$here/state/session-$(date -u +%Y%m%dT%H%M%SZ)-$$
    else
        state=$here/state
    fi
fi
mkdir -p "$state"
chmod 700 "$state" 2>/dev/null || true
config=$state/config.toml
cat > "$config" <<EOF
version = 1
socket = "$state/engine.sock"
state = "$state"

[apps]
require-signature = $REQUIRE_SIGNATURE
allow-unverified-assets = $ALLOW_UNVERIFIED

[services]
allow-third-party = $ALLOW_THIRD_PARTY

[limits]
start-timeout-ms = 5000
EOF

sock=$state/engine.sock
panel=
cleanup() {
    if [ -n "$panel" ]; then
        kill "$panel" 2>/dev/null || true
        wait "$panel" 2>/dev/null || true
    fi
    if [ "$ephemeral" = yes ]; then
        rm -rf "$state"
    fi
}
trap cleanup EXIT INT TERM

"$here/bin/anth-panel" --listen "$listen" --config "$config" > "$state/panel.log" 2>&1 &
panel=$!
wait_for_engine() {
    tries=0
    while [ "$tries" -lt 150 ]; do
        if "$here/bin/anth" --socket "$sock" status >/dev/null 2>&1; then
            return 0
        fi
        tries=$((tries + 1))
        sleep 0.1
    done
    return 1
}
if ! wait_for_engine; then
    echo "the panel did not come up; read $state/panel.log" >&2
    exit 1
fi

if [ "$MODE" != core ]; then
    anth="$here/bin/anth --socket $sock"
    installed=$($anth app list | cut -f1 || true)
    for app in $APPS; do
        id=org.anthracite.$app
        if ! printf '%s\n' "$installed" | grep -qx "$id"; then
            $anth app install "$here/bundles/$app.anth" >/dev/null
        fi
        case " $HOST_APPS " in
            *" $app "*)
                $anth app approve "$id" --accept-write --accept-admin --accept-host >/dev/null
                ;;
            *)
                $anth app approve "$id" --accept-write --accept-admin >/dev/null
                ;;
        esac
    done
    set --
    for app in $APPS; do
        set -- "$@" --add "org.anthracite.$app"
    done
    $anth compose "$@" --now >/dev/null
    if ! wait_for_engine; then
        echo "the panel did not come back after composing; read $state/panel.log" >&2
        exit 1
    fi
fi

echo "Anthracite $MODE $VERSION on http://$listen"
echo "state: $state"
echo "operator password: $state/credentials.txt (created on first boot)"
if [ "$MODE" = base ] && [ "$(id -u)" != 0 ]; then
    echo "note: net and firewall need root or CAP_NET_ADMIN; they may start degraded"
fi
echo "Ctrl-C stops the panel; the state stays in $state"
wait "$panel"
RUN
    chmod 755 "$1/run.sh"
}

variant_manifest() {
    # $1 dir, $2 variant, $3 file prefix (bundles/<app>.anth), $4 signed yes|no
    {
        printf '{\n'
        printf '  "variant": "%s",\n' "$2"
        printf '  "version": "%s",\n' "$version"
        printf '  "platform": "%s",\n' "$platform"
        printf '  "bundles_signed": %s,\n' "$4"
        printf '  "distribution_key": {"public_key": "%s", "fingerprint": "%s"},\n' "$public" "$fingerprint"
        printf '  "apps": ['
        first=1
        for app in $system_apps; do
            case "$2" in
                core) break ;;
            esac
            if [ "$first" = 1 ]; then
                first=0
                printf '\n'
            else
                printf ',\n'
            fi
            printf '    {"id": "org.anthracite.%s", "file": "%s/%s.anth"}' "$app" "$3" "$app"
        done
        if [ "$first" = 0 ]; then
            printf '\n  '
        fi
        printf ']\n}\n'
    } > "$1/manifest.json"
}

echo "variants: staging core"
mkdir -p "$out/core/bin"
cp "$release"/bin/anth "$release"/bin/anth-engine "$release"/bin/anth-panel "$out/core/bin/"
cat > "$out/core/variant.conf" <<EOF
MODE=core
VERSION=$version
APPS=
HOST_APPS=
REQUIRE_SIGNATURE=true
ALLOW_UNVERIFIED=false
ALLOW_THIRD_PARTY=false
SESSION_STATE=no
EOF
run_script "$out/core"
cat > "$out/core/README.md" <<EOF
# Anthracite $version — core

The panel, the CLI and the engine with **no apps**: an empty composition, no
bundles, no services. This is the smallest supported deployment and the base the
other variants add to.

## Run

\`\`\`sh
./run.sh                              # http://127.0.0.1:8090
./run.sh --listen 127.0.0.1:5176      # another address
\`\`\`

The first boot writes the operator password to \`state/credentials.txt\`; sign in
as \`operator\` with it and change it. The state stays in \`state/\` across
restarts; remove that directory to start over.

## Verify

\`\`\`sh
sha256sum -c SHA256SUMS
./bin/anth --version
./bin/anth --socket state/engine.sock key distribution   # while run.sh is up
\`\`\`

The engine trusts the distribution key \`$fingerprint\`. Install bundles later with
\`bin/anth app install\`; nothing is preinstalled here.
EOF
variant_manifest "$out/core" core bundles false

echo "variants: staging base"
mkdir -p "$out/base/bin" "$out/base/bundles"
cp "$release"/bin/anth "$release"/bin/anth-engine "$release"/bin/anth-panel "$out/base/bin/"
for app in $system_apps; do
    [ -f "$release/bundles/$app.anth" ] || { echo "the release has no $app bundle" >&2; exit 1; }
    cp "$release/bundles/$app.anth" "$out/base/bundles/"
done
cat > "$out/base/variant.conf" <<EOF
MODE=base
VERSION=$version
APPS="$system_apps"
HOST_APPS="accounts firewall fs home net storage terminal"
REQUIRE_SIGNATURE=true
ALLOW_UNVERIFIED=false
ALLOW_THIRD_PARTY=false
SESSION_STATE=no
EOF
run_script "$out/base"
cat > "$out/base/README.md" <<EOF
# Anthracite $version — base

The panel, the CLI and the engine plus the eight **signed system apps**:
accounts, firewall, fs, home, net, proc, storage and terminal. They install as
\`system\`; the engine trusts the distribution key \`$fingerprint\`.

## Run

\`\`\`sh
./run.sh                              # installs, approves, composes and serves on 127.0.0.1:8090
./run.sh --listen 127.0.0.1:5176
\`\`\`

\`run.sh\` is idempotent: it installs what is missing, approves the bundles with the
levels their class requires, composes them and asks the engine to restart. The
real \`net\` and \`firewall\` services need root or \`CAP_NET_ADMIN\`; without it they
start degraded. The operator password is generated on first boot at
\`state/credentials.txt\`.

## Verify

\`\`\`sh
sha256sum -c SHA256SUMS
./bin/anth --socket state/engine.sock app list
./bin/anth --socket state/engine.sock app show org.anthracite.fs   # "tier": "system"
\`\`\`

The state persists in \`state/\`; remove it to start over. Keep the state path
short: every service socket lives under it and Linux caps a Unix socket path at
107 bytes, so a deeply nested checkout can degrade every native app.
EOF
variant_manifest "$out/base" base bundles true

echo "variants: staging demo"
mkdir -p "$out/demo/bin" "$out/demo/bundles"
cp "$release"/bin/anth "$release"/bin/anth-engine "$release"/bin/anth-panel "$out/demo/bin/"
[ -d "$release/demo" ] || { echo "the release has no signed demo bundles" >&2; exit 1; }
for app in $system_apps; do
    [ -f "$release/demo/$app.anth" ] || { echo "the release has no demo $app bundle" >&2; exit 1; }
    cp "$release/demo/$app.anth" "$out/demo/bundles/"
done
cat > "$out/demo/variant.conf" <<EOF
MODE=demo
VERSION=$version
APPS="$system_apps"
HOST_APPS="home"
REQUIRE_SIGNATURE=true
ALLOW_UNVERIFIED=false
ALLOW_THIRD_PARTY=false
SESSION_STATE=yes
EOF
run_script "$out/demo"
cat > "$out/demo/README.md" <<EOF
# Anthracite $version — demo

The same panel and the same app ids as **base**, but the eight bundles are the
deterministic **host emulator**: no real files, processes, network or accounts.
The emulator bundles are signed by the release key like the system apps, so this
variant keeps the strict configuration (\`require-signature = true\`,
\`allow-third-party = false\`) and runs without root.

## Run

\`\`\`sh
./run.sh                              # a new session, 127.0.0.1:8090
./run.sh --state my-demo              # resume a previous session
./run.sh --listen 127.0.0.1:5176 --ephemeral
\`\`\`

Every run without \`--state\` creates \`state/session-<timestamp>-<pid>\` and keeps
it after exit: files, users, routes and mounts you change in the demo persist for
that session and are never shared with another one. \`--ephemeral\` deletes the
session on exit.

The operator password is in \`<state>/credentials.txt\`; the panel prints the exact
path.

## Verify

\`\`\`sh
sha256sum -c SHA256SUMS
./bin/anth --socket state/session-*/engine.sock app home info
\`\`\`

The emulator writes its snapshot to \`{shared}/emulator/host.json\` inside the
session state; \`{shared}\` is the service trust domain. Keep the session state
path short: every service socket lives under it and Linux caps a Unix socket
path at 107 bytes.
EOF
variant_manifest "$out/demo" demo bundles true

echo "variants: staging demo-sessions"
cargo build --manifest-path demo/sessions/Cargo.toml --release
mkdir -p "$out/demo-sessions/bin" "$out/demo-sessions/bundles"
cp "$release"/bin/anth "$release"/bin/anth-engine "$release"/bin/anth-panel "$out/demo-sessions/bin/"
cp ./target/release/anth-demo-sessions "$out/demo-sessions/bin/"
[ -d "$release/demo" ] || { echo "the release has no signed demo bundles" >&2; exit 1; }
for app in $system_apps; do
    cp "$release/demo/$app.anth" "$out/demo-sessions/bundles/"
done
cat > "$out/demo-sessions/run.sh" <<'RUN'
#!/bin/sh
# One panel and one expired demo state per browser session. No password: the
# launcher signs the visitor in. A public launcher requires a CAPTCHA.
set -eu
here=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
listen=${ANTHRACITE_LISTEN:-127.0.0.1:8091}
exec "$here/bin/anth-demo-sessions" \
    --panel "$here/bin/anth-panel" \
    --cli "$here/bin/anth" \
    --bundles "$here/bundles" \
    --state "$here/state" \
    --listen "$listen" \
    --max "${ANTHRACITE_MAX_SESSIONS:-8}" \
    --ttl-seconds "${ANTHRACITE_DEMO_TTL:-1800}" \
    --port-base "${ANTHRACITE_PORT_BASE:-0}" \
    --port-count "${ANTHRACITE_PORT_COUNT:-100}" \
    "$@"
RUN
chmod 755 "$out/demo-sessions/run.sh"
cat > "$out/demo-sessions/README.md" <<EOF
# Anthracite $version — demo, one ephemeral state per browser session

Like **demo**, with one panel and one emulator state **per browser session**. The
launcher keeps a cookie that maps a browser to its own panel on its own port, so
two visitors never see each other's files, users, routes or mounts. The eight
bundles are the signed host emulator; the configuration is strict
(\`require-signature = true\`, \`allow-third-party = false\`).

There is **no password**: the launcher changes the panel's generated password to
a random one it never shows, signs the visitor in through the engine's session
endpoint and forwards the session cookie. Sessions last
\`ANTHRACITE_DEMO_TTL\` seconds (1800 by default); when one expires the launcher
kills its panel and **deletes its state**.

## Turnstile and abuse limits

A launcher that is not loopback, or that sets \`--public-host\`, refuses to start
without Cloudflare Turnstile and the expected hostname. Set:

\`\`\`sh
ANTHRACITE_CAPTCHA_SITE_KEY=<site key>      # public; can also be --captcha-site-key
ANTHRACITE_CAPTCHA_SECRET=<secret>          # env only, never argv
ANTHRACITE_CAPTCHA_HOSTNAME=demo.internal   # required for a public launcher
ANTHRACITE_TRUST_PROXY=1                    # read CF-Connecting-IP / X-Forwarded-For
\`\`\`

The landing page renders the Turnstile widget with \`action=demo-session\`; the
launcher verifies success, action and hostname server-side before creating
anything, and checks \`Origin\`/\`Referer\` on the form post. On a loopback
launcher Turnstile is optional, which is what makes local development work
without keys. \`--captcha-verify-url\` overrides the endpoint for a local verifier.

Abuse limits: three sessions per address per hour, six per minute and sixty per
hour globally, ten attempts per address per minute, and five failed challenges in
ten minutes block that address for an hour. \`ANTHRACITE_MAX_SESSIONS\` caps the
live sessions. The proxy should overwrite \`X-Forwarded-For\` (or send
\`CF-Connecting-IP\`) so the per-address limits see the visitor, not the proxy.

## Run

\`\`\`sh
./run.sh                                   # landing page on http://127.0.0.1:8091
ANTHRACITE_LISTEN=127.0.0.1:9000 ./run.sh  # another address
./run.sh --public-host demo.internal       # when a proxy or tunnel fronts it
\`\`\`

Open the landing page, solve the challenge and the panel opens already signed in.
Reloading or reopening the browser returns to the same session until it expires;
the state is then gone for good.

The launcher binds loopback by default. For the public demo, the operator's proxy
terminates TLS and forwards to the launcher and to the per-session child ports;
the launcher reads \`X-Forwarded-Proto\` and \`Host\` so redirects and cookies use
the public scheme and host. Service sockets live under the state root and Linux
caps a Unix socket path at 107 bytes, so keep that root short.

The release also builds a Docker image of this variant
(\`scripts/release-images.sh\`): it exposes 8091 and 8100-8199, sets
\`ANTHRACITE_PORT_BASE=8100\` and needs the Turnstile variables at run time.

## Verify

\`\`\`sh
sha256sum -c SHA256SUMS
ls state/sessions                     # one directory per live session
\`\`\`

Each session's state is a complete engine tree: \`state/sessions/<id>/state\` holds
its apps, grants, audit and the emulator snapshot at
\`state/sessions/<id>/state/shared/emulator/host.json\`.
EOF
variant_manifest "$out/demo-sessions" demo-sessions bundles true

echo "variants: checksums and archives"
for variant in core base demo demo-sessions; do
    (
        cd "$out/$variant"
        if [ "$variant" = core ]; then
            sha256sum bin/* manifest.json variant.conf run.sh README.md > SHA256SUMS
        elif [ "$variant" = demo-sessions ]; then
            sha256sum bin/* bundles/* manifest.json run.sh README.md > SHA256SUMS
        else
            sha256sum bin/* bundles/* manifest.json variant.conf run.sh README.md > SHA256SUMS
        fi
    )
done

(
    cd "$out"
    for variant in core base demo demo-sessions; do
        tar -czf "anthracite-$version-$variant-$platform.tar.gz" "$variant"
    done
    sha256sum anthracite-*.tar.gz > SHA256SUMS
)

{
    printf '# Anthracite %s pre-release — core, base, demo and demo-sessions\n\n' "$version"
    printf 'Four variants built from the same signed release. The distribution key fingerprint is `%s`; every bundle in `base` and `demo` (including the signed host emulator) is `system`-signed, and the binaries in all four trust that key.\n\n' "$fingerprint"
    printf '| Variant | Contents | Start |\n| --- | --- | --- |\n'
    printf '| `core` | panel, CLI, engine, **no apps** | `./run.sh` |\n'
    printf '| `base` | core + the eight signed system apps | `./run.sh` (installs and composes) |\n'
    printf '| `demo` | core + the signed host-emulator bundles, one state per run | `./run.sh` |\n'
    printf '| `demo-sessions` | demo + one ephemeral, CAPTCHA-gated session per browser | `./run.sh` (landing page) |\n\n'
    printf 'Each archive contains `bin/`, `run.sh`, a `README.md`, a `manifest.json` and `SHA256SUMS`; `base`, `demo` and `demo-sessions` also carry `bundles/`. Verify with `sha256sum -c SHA256SUMS` (and again over this file for the tarballs).\n\n'
    printf 'Platform: `%s`. These are pre-release artifacts: GitHub Actions has had no runner, the native services have not been exercised on a deployment host, and the key is the pre-release key, not a blessed long-lived release key.\n' "$platform"
} > "$out/RELEASE-NOTES.md"

echo "variants: wrote $out"
echo "variants: $out/anthracite-$version-{core,base,demo,demo-sessions}-$platform.tar.gz"
