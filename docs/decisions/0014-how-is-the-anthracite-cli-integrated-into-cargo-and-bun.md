# How is the Anthracite CLI integrated into cargo and Bun?

- Date: 2026-11-25T18:56:46.743Z
- Feature: app-sdk (`app-sdk`)
- Status: accepted

## Question

How is the Anthracite CLI integrated into cargo and Bun?

## Decision

One Rust crate, cargo-anth, installs the binaries cargo-anth and anth. cargo anth uses cargo's cargo-<subcommand> discovery, like clippy or nextest. Bun uses bunx @anth-panel/cli, bun create @anth-panel/app and an optional bunfig preload plugin; there is no native bun anth subcommand.

## Rationale

Cargo has a documented extension point; Bun does not. The Bun integration mirrors Drizzle: a package binary run with bunx plus a scaffolder and a plugin, instead of a shell alias or a patched Bun.

## Alternatives considered

- A separate anth-dev binary; rejected as a second install and a second documentation path.
- A shell wrapper named bun-anth; rejected because it is not installable or portable.

## References

- Repository: janitochvvll2311/anthracite-panel
- Recorded by: feature-implementor
