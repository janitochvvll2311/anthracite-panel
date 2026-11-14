# Report — The operator command

## Summary

`/api/engine/operator` had a `GET` and a `PATCH` and no CLI. `anth operator show`
and `anth operator password` close the gap without putting a password in argv:
the CLI prompts on the terminal with echo off, or reads one line from stdin for a
deployment script.

## Delivered

- `anth operator show` → `GET /api/engine/operator`: prints the authenticated
  identity (`uid:<n>` over the socket, the operator name over a browser session).
- `anth operator password` → `PATCH /api/engine/operator`: sends
  `{"new_password": …}`. Over the Unix socket the peer credential is proof of
  identity, which the API already encodes (`AuthKind::Peer`); nothing asks for the
  old password there, and the server still enforces its own policy (minimum
  length, must differ) and revokes every live session.
- The password is read from the terminal with echo disabled and restored even
  when the read fails, or from stdin when it is not a terminal. It never appears
  in argv, the process list or the shell history.
- `Client::json` now sends `Content-Type: application/json` when the body is not
  empty. The new command is the first PATCH through that helper, and without the
  header the engine answers `415` — a trap for every future caller.
- Docs: §7.3 lists the command against its resource, the parity report moves the
  row to *pass* and narrows `anth login/logout` to the transport question it
  really is.

## Tests

- `sdk/rust/cli`: **64 passed** (4 new):
  - `operator_takes_exactly_its_two_subcommands` (no subcommand, extra argument,
    an argv password and an unknown verb are refused),
  - `operator_show_reads_the_identity_route`,
  - `an_operator_password_is_a_patch_and_never_argv` (method, path, body and
    content type),
  - `an_empty_operator_password_is_refused_before_the_request`.
- Manual end to end against a real panel: `anth operator show`, a password piped
  through stdin, login with the new password `200` and with a wrong one `401`.

## Decisions

- **Never take the password on argv.** `--password <value>` is convenient and
  shows up in `ps`; the terminal prompt and stdin are the two safe inputs.
- **No current password over the socket.** That is the API's existing rule for a
  peer credential, not a new exception; a browser session still has to prove it.
- **`Client::json` owns the content type.** A helper named `json` that sends a
  JSON body without saying so is a bug waiting for the next command.

## Out of scope

- `anth login`/`logout`: the CLI speaks to the engine's Unix socket, where the
  peer credential is the session; a remote client is a different transport.
- `anth restart`: a separate PR with the API route and the process transition.

## Pending and risks

- The terminal prompt writes to stderr and reads stdin; a shell that redirects
  stdin to `/dev/null` gets an empty password and an explicit refusal.
- The identity over the socket is `uid:<n>`, not the operator name, which is what
  the API reports for a peer; the name belongs to the browser session.
