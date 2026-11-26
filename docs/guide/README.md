# The Anthracite guide

This is the official manual for Anthracite **0.7**. It describes how to install,
configure and operate the engine and the official panel, how to write apps,
services and panels, and how to sign and release bundles.

The design of record lives separately: [`DESIGN.md`](../../DESIGN.md) and the
[`docs/design/`](../design/) documents explain *why* the platform is shaped this
way, and their decisions are binding. The guide links to them where the reason
matters; it does not repeat the whole design.

The `v0.8.0` line adds the third-party SDK (`cargo anth` / `anth` and
`bunx @anth-panel/cli`). Its contract is
[§31](../design/31-sdk-distribution.md); the install commands are documented
when the packages are published.

## Reading paths

**Operator.** [Installation](installation.md) → [Configuration](configuration.md)
→ [CLI](cli.md) / [HTTP API](http-api.md) → [Signing](signing.md) →
[Operations](operations.md) → [Troubleshooting](troubleshooting.md).

**App author.** [App SDK](sdk.md) → [Apps](apps.md) → [Signing](signing.md) →
[Services](services.md) if the app needs host reach → [Panels](panels.md) for a
UI → the [conformance kit](apps.md#the-conformance-kit).

**Edge operator.** [Edge apps](#edge-apps) → [Ports, roots and capabilities](ports.md)
→ [Edge composition QA](edge-qa.md) → [Release notes](release-notes.md).

**Panel author.** [Panels](panels.md) → [HTTP API](http-api.md) →
[Apps](apps.md) (the surface and slot shapes).

**Contributor.** [`AGENTS.md`](../../AGENTS.md) → [`DESIGN.md`](../../DESIGN.md)
→ [`docs/design/`](../design/) → the feature reports under
[`docs/features/`](../features/).

## Pages

| Page | Covers |
| --- | --- |
| [Getting started](getting-started.md) | a running panel in minutes, from a release or from source |
| [Installation](installation.md) | release variants and images, source builds, services, upgrades |
| [Configuration](configuration.md) | every configuration key, profiles, state layout, environment |
| [CLI](cli.md) | every `anth` command, flags, exit codes |
| [HTTP API](http-api.md) | authentication, errors, resources, streaming and assets |
| [Apps](apps.md) | bundle layout, manifests, WIT, routes, slots, approval, upgrades |
| [App SDK](sdk.md) | cargo anth, bunx, anth.toml, templates, compatibility and troubleshooting |
| [Services](services.md) | native services, classes, placeholders, supervision, sandbox |
| [Signing](signing.md) | tiers, keys, `anth-sign`, the release ceremony, verification |
| [Panels](panels.md) | the panel contract, slots, the iframe bridge, building a panel |
| [Demo](demo.md) | the emulator, the demo variants, ephemeral sessions, images |
| [Operations](operations.md) | state, backup, audit, logs, restart, upgrades |
| [Security](security.md) | the authority model, secrets, what is and is not enforced |
| [Troubleshooting](troubleshooting.md) | symptoms, causes, fixes |
| [Glossary](glossary.md) | the vocabulary used everywhere else |
| [Release notes](release-notes.md) | 0.7.0 changes, upgrade and limitations |
| [Edge composition QA](edge-qa.md) | the manual matrix behind the release gate |

## Edge apps

The five business apps of `v0.7.0`; together with the system apps they form the
edge composition in [`compositions/edge.toml`](../../compositions/edge.toml).

| Page | Covers |
| --- | --- |
| [Certificates](certs.md) | inventory, import, validate and publish |
| [ACME](acme.md) | accounts, HTTP-01, DNS-01, renewal and revocation |
| [Reverse proxy](proxy.md) | vhosts, TLS, WebSocket, HTTP/2 and apply/rollback |
| [Containers](containers.md) | lifecycle, logs, stats and exec |
| [Store](store.md) | signed indexes, downloads and the manager handoff |
| [Interface reference](interfaces.md) | the WIT functions of the five services |
| [Ports, roots and capabilities](ports.md) | manifest v4 reach, sandbox L1 and approval text |

## Conventions

- Shell examples use `sh` and assume `anth` is on `PATH`. Set `ANTH_SOCKET` or
  pass `--socket <path>` when the engine does not use the default
  `/run/anth/engine.sock`.
- `$state` is the engine's state directory (default `/var/lib/anth`).
- `0.7.x` means the 0.7 release line; the exact version is `anth --version`.
- Links into [`docs/design/`](../design/) explain a reason. If this guide and a
  design decision disagree, the design decision wins and the guide is wrong:
  report it.
