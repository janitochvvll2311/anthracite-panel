# Release packaging

`v0.3.0a` is a pre-release marker: the code and crate version remain `0.3.0`
until the next tagged build. The suffix is in the tag, tarball and release title
so an operator can tell this build from the next one.

```bash
./scripts/package.sh          # version 0.3.0a
./scripts/package.sh 0.3.0b   # a later pre-release
```

The script:

1. installs frontend dependencies, builds the shell and stages `www/`;
2. builds `anth-panel` for `x86_64-unknown-linux-musl` in release mode;
3. creates a release-default `config.toml.example` with the binary itself;
4. stages the binary, unit, docs, reference extension sources and Dockerfile;
5. writes `dist/anthracite-panel-<version>-<target>.tar.gz` and its SHA-256.

The tarball layout:

```
anthracite-panel-0.3.0a-x86_64-unknown-linux-musl/
  bin/anth-panel
  etc/anth/config.toml.example
  lib/systemd/system/anthracite-panel.service
  share/containers/Dockerfile
  share/doc/anthracite-panel/{README.md,LICENSE,docs/...,extensions/...}
```

## Install

```bash
tar -xzf anthracite-panel-0.3.0a-x86_64-unknown-linux-musl.tar.gz
sudo install -m755 anthracite-panel-0.3.0a-*/bin/anth-panel /usr/local/bin/anth-panel
sudo install -d /etc/anth /var/lib/anth
sudo install -m644 anthracite-panel-0.3.0a-*/etc/anth/config.toml.example /etc/anth/config.toml
sudo install -m644 anthracite-panel-0.3.0a-*/lib/systemd/system/anthracite-panel.service \
  /etc/systemd/system/anthracite-panel.service
sudo systemctl daemon-reload
sudo systemctl enable --now anthracite-panel
```

The release config has auth enabled and feature switches off. Edit
`/etc/anth/config.toml` or use `anth-panel config set`; the daemon needs a
restart for policy changes.

## Container

From the unpacked release directory:

```bash
docker build --build-arg VERSION=0.3.0a -t anthracite-panel:0.3.0a .
docker run --rm -p 8080:8080 \
  --cap-add=NET_ADMIN --cap-add=SYS_ADMIN \
  -v anth-config:/etc/anth -v anth-state:/var/lib/anth \
  anthracite-panel:0.3.0a
```

The image bundles `iproute2`, `nftables`, `util-linux`, `shadow` and
`wireguard-tools`. Capabilities are only needed for the features that use them;
without `NET_ADMIN`/`SYS_ADMIN` the inventories still work and mutations are
refused with a permission error.

## CI

`.github/workflows/release.yml` runs on `v*` tags (or manually). It builds the
same tarball, uploads it as an artifact and attaches it to a **prerelease**
GitHub release. A tag is never moved once published; make a new suffix instead.

## Demo image

The hosted demo has its own tarball and image; it is not the privileged release:

```bash
./scripts/package-demo.sh 0.3.0a
```

```
anthracite-panel-demo-0.3.0a-x86_64-unknown-linux-musl/
  bin/anth-panel-demo
  etc/anth/config.toml
  share/containers/Dockerfile
  share/doc/anthracite-panel/{README.md,LICENSE,docs/...}
```

The binary is compiled with `--features demo`, which forces the demo backends
even if `ANTH_BACKEND=real` is set. The image runs non-root without capabilities
and expects a reverse proxy for TLS; see `docs/demo.md`.
