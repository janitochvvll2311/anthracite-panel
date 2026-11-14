# CLI

`anth` is a thin client for the engine's Unix socket. It does not link the
daemon; commands become HTTP requests, and the engine does the deciding. The
same rules apply to the API ([HTTP API](http-api.md)); the CLI is a convenience,
not a second implementation.

```sh
anth [--socket <path>] <command> [options]
```

The socket is `--socket`, then `ANTH_SOCKET`, then `/run/anth/engine.sock`. The
CLI is Unix-socket only: it authenticates with the peer credential, so there is
no login. A remote engine is reached with the HTTP API and a session.

## Commands

### `doctor`

```sh
anth doctor [--config <path>] [--debug|--release]
```

Runs the engine binary with `--check`: configuration, state and composition are
validated without starting a daemon. The engine is looked up next to `anth` or
through `ANTH_ENGINE`. It is the command that saves you when the engine does not
start.

### `status`

```sh
anth status
```

Prints the engine version and protocol, readiness, the composition and the
installed catalogue. Readiness is `starting` while the daemon boots apps.

### `compose`

```sh
anth compose                    # show the document
anth compose --show
anth compose --add <id> [--add <id> ...]
anth compose --remove <id> [--remove <id> ...]
anth compose --add <id> --now
```

The composition is replaced as a whole document with an `If-Match` validator;
`--add` and `--remove` are client-side edits. `--now` saves the document and then
asks the engine to restart; a refused restart is a warning, because the write
already succeeded. Running apps change on the restart (`anth restart`).

### `audit`

```sh
anth audit [--limit <n>] [--before <n>]
```

Newest first; the limit defaults to 100 and cannot exceed 1000. `--before` is
the pagination cursor. Denials are recorded even when nothing changed.

### `restart`

```sh
anth restart
```

`POST /api/engine/restart`: the host stops serving, stops apps and services and
re-execs itself with the same arguments. The answer is `202 Accepted` with
`Location: /api/health`; the pid survives, and readiness is `anth status`.

### `key`

```sh
anth key list
anth key add <public-key> [--label <label>]
anth key remove <fingerprint>
anth key distribution
```

The keyring holds **third-party** signers; everything it trusts is
`third-party`. `key distribution` prints the key this build trusts as `system`
(the compiled distribution key), or `none` for a development build. It exits
non-zero if the build carries an unusable key. Public keys are 32 bytes of
lowercase hex; the engine derives and prints the fingerprint.

### `operator`

```sh
anth operator show
anth operator password
```

`show` prints the authenticated identity (`uid:<n>` over the socket, the operator
name over a browser session). `password` reads the new password from the terminal
with echo off, or one line from stdin when stdin is not a TTY; it is never taken
from argv. Over the socket the peer credential is the proof, so the current
password is not asked. Policy is the engine's: minimum length, must differ, and
every live session is revoked.

### `app`

```sh
anth app list
anth app show <id>
anth app install <bundle.zip>
anth app upgrade <id> <bundle.zip>
anth app approve <id> [--accept-write|--accept-admin|--accept-host|--accept-package]
anth app revoke <id> [--acknowledge <token>]
anth app enable|disable <id> [--acknowledge <token>]
anth app remove <id> [--acknowledge <token>]
anth app fetch-wit <id> [--out <dir>]
```

- `install` derives the id from the bundle; a second install of the same id is a
  conflict. `upgrade` replaces the bundle of a known id and revokes the grant.
- `approve` freezes the grant. The flags must cover the computed risk: `read`
  needs none; `write`, `admin`, `host` and `package` each add a flag. Use
  `app show <id>` (and the approval preview in the API/panel) to see the risk and
  the exact interfaces.
- `enable`, `disable` and `remove` ask for an acknowledgment when composed apps
  depend on the target: the engine returns the `dependents` token, and
  `--acknowledge <token>` (the CLI's `--acknowledge dependents`) confirms it. The
  acknowledgment is audited.
- `fetch-wit` writes the installed provider's WIT under `wit/deps/<package>` by
  default; `--out` chooses another directory.
- `show` prints the manifest, descriptor, grant and risk as JSON; `list` prints
  one row per app (id, version, state, alias, grant state).

### App verbs

```sh
anth app <alias|id> <verb> [--flag <value>] [--flag] [--no-flag]
```

A `[[cli]]` entry names an interface function; the engine resolves the app's
`[[route]]` bound to the same function and calls it through the audited route.
`--flag value` sets a parameter, a bare `--flag` sets `true`, and `--no-flag`
sets `false`. An app that is not composed has no verbs; an unknown verb is
`unavailable`, not a silent call.

## Output and exit codes

- Human-readable results go to stdout; warnings and errors go to stderr. Errors
  are printed as `<code>: <message>`, and a session that expired says so.
- Exit codes match the engine's error kinds:

| Code | Kinds |
| --- | --- |
| `0` | success |
| `1` | internal error |
| `2` | invalid, conflict, precondition failed, method not allowed |
| `3` | denied, unauthenticated, password change required, rate limited |
| `4` | not found |
| `5` | unavailable, quota exhausted |
| `6` | payload too large |

`driver` commands (`doctor`) return the engine binary's exit code unchanged.

## Examples

```sh
# install, approve and compose a signed bundle
anth app install ./notes.zip
anth app approve org.example.notes --accept-write
anth compose --add org.example.notes --now

# trust a third-party signer and inspect the build key
anth key add ea4a... --label "notes publisher"
anth key distribution

# a generated verb
anth app notes create --title "Groceries"

# what happened
anth audit --limit 20
anth status
```
