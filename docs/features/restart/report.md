# Report — Restart

## Summary

`POST /api/engine/restart` and `anth restart` existed in the design and in
neither the binary. The engine now asks its host to replace the process, the
daemon and the panel re-exec with the same arguments, and `compose --now` uses
the route instead of telling the operator to go restart something by hand.

## Delivered

- `POST /api/engine/restart` answers `202 Accepted` with `Location: /api/health`,
  audits `engine.restart` with the pid, and never claims readiness. The router
  only asks: `EngineState::with_restart` takes an `anth_api::Restart` hook, so a
  host that does not own its process answers `503` rather than pretending.
- The daemon and the linked panel implement the hook on a `watch` channel that
  already drives their graceful shutdown; after the servers drain and every app
  and service is stopped, the process re-execs itself with the same arguments.
  The pid survives, so a service manager does not see a stop/start pair.
- Both TCP listeners now bind with `SO_REUSEADDR`: the old process's connections
  can be in `TIME_WAIT` when the new image binds the same port.
- `anth restart` posts the route and prints the accepted `Location`. `compose
  --now` saves the document, asks for the restart and reports a refusal without
  failing the command, because the document is already saved.
- `AuditAction::EngineRestart` (`engine.restart`) is part of the pinned action
  list; §6.8's list and §7.3's command table name it.

## Tests

- `engine/api`: **98 passed** (2 new): the route asks the host once, answers
  `202` with `Location: /api/health` and an audit entry; a host without a process
  gets `503`.
- `engine/daemon`: a restart request reaches the stop channel as `Restart` (the
  unit that the async path uses), plus the whole workspace green.
- `sdk/rust/cli`: **67 passed** (3 new): `restart` is a bare command, it posts
  the route, and `compose --now` sends the GET, the PUT and then the restart.
- Manual, both binaries: `anth restart` against the daemon and against the panel;
  same pid after the restart, the socket rebinds and `anth status` reports
  `ready`; `compose --add hello --now` restarts and the app answers
  (`hello, Ada (at … ms)`).

## Decisions

- **The host owns the process.** The router cannot re-exec itself, and a test or
  a host that only mounts the API should not pretend it can; `503` is the honest
  answer there.
- **Re-exec, not fork.** The pid and the arguments survive, which is the
  behaviour §6.5 promises and what a service manager expects.
- **Restart after an orderly stop.** Services and instances stop first, so a
  native service does not lose its socket or its children to an abrupt image
  swap.
- **`--now` is best effort.** The composition document is already written; a
  refused restart is a warning, not an error that hides the successful write.

## Out of scope

- Hot reload: the composition still takes effect on restart, which is now one
  command away.
- `anth login/logout` over HTTP; the CLI remains a Unix-socket client.
- Restarting a single app through the API.

## Pending and risks

- A restart during a long service stop delays the new image; the API answered
  `202` before that, so a client must poll readiness (`anth status`).
- Re-exec uses the binary path from `current_exe`; a deployment that replaces the
  binary on disk between boots gets the new file, which is the useful behaviour
  but worth knowing.
