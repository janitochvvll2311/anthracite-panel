# Firewall

Firewall is a native host service plus the sandboxed panel view. It is a
separate bundle from Network; its `panel.view` slot names
`org.anthracite.net#anth-net` as parent, so the panel renders it as a sub-tab
under Network without giving either app access to the other.

- `backend/` owns exactly one nftables table, `inet anthracite`, with `input`
  and `output` base chains. It shells out to `nft` by argv, never through a
  shell, and ignores every other table.
- Managed rules carry `anth:<action>:<protocol>:<port>[|<comment>]`. A comment
  the panel did not write is a custom rule: it is listed and can be deleted, but
  not edited.
- Mutations are gated by `--firewall-control` in the bundle argv, visible at
  approval. Without it the snapshot is read-only.
- `snapshot`, `add-rule`, `update-rule`, `delete-rule`, `flush` and
  `set-policy` make up `anthracite:firewall@1.0.0`.
- Adding a rule for a target that is already managed replaces it in place, so a
  chain/protocol/port never has two conflicting verdicts.
- A chain policy of `deny` drops every packet no rule accepted, including the
  panel. The view asks for confirmation and the README keeps the warning in
  sight.

## Build

```bash
apps/firewall/build.sh
# writes apps/firewall/dist/firewall.anth
```

## Tests

```bash
cargo test --manifest-path apps/firewall/backend/Cargo.toml
cargo clippy --manifest-path apps/firewall/backend/Cargo.toml --all-targets -- -D warnings
```

The service uses an injectable `NftRunner`; tests assert the exact argv and
parse a fixed `nft -j` table. They never touch the host firewall. Real
validation needs a disposable container with `--cap-add=NET_ADMIN` and `nft`
installed.
