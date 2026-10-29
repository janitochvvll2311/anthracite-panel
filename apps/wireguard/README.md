# WireGuard

A read-only reference app. It lists the interfaces `wg show interfaces`
reports and shows one interface's human output. Both commands are frozen
capabilities; the guest cannot add a flag or a second command.

Creation, deletion, peer changes and config persistence are not in this cut:
they need host-owned jobs, private-key handling and a config writer. Keeping
this bundle read-only makes the trust boundary visible instead of implying
authority the app does not have.

```bash
./package.sh
```

The script writes `dist/org.anth.wireguard.zip` and prints the Ed25519 public
key to set as `[apps] public_key`.
