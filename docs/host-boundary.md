# Host boundary and naming

`anth-host` owns machine access. `anth-panel` owns policy, application flow and
transports. The names are deliberately mechanical so a new feature does not
invent a fourth convention.

## The rule

| Piece | Name | Example |
| --- | --- | --- |
| Trait/port | `<Feature>Service` in `anth_host::backend` | `NetworkService`, `FilesService` |
| Real implementation | `System<Feature>` in `anth_host::<feature>` | `SystemNetwork`, `SystemFiles` |
| Demo implementation | `Demo<Feature>` in `anth_host::demo::<feature>` | `DemoNetwork`, `DemoFirewall` |
| Module | the feature name, lowercase | `network`, `firewall`, `files`, `process`, `terminal` |
| Panel field | `Arc<dyn <Feature>Service>` | `Arc<dyn NetworkService>` |

`Service` therefore means the **port**, not a concrete struct. The concrete
implementations are always `System*` or `Demo*`.

## Current map

| Feature | Trait | Real | Demo |
| --- | --- | --- | --- |
| Network | `NetworkService` | `SystemNetwork` | `DemoNetwork` |
| Firewall | `FirewallService` | `SystemFirewall` | `DemoFirewall` |
| Files | `FilesService` | `SystemFiles` | `DemoFiles` |
| Processes | `ProcessesService` | `SystemProcesses` | `DemoProcesses` |
| Volumes | `VolumesService` | `SystemVolumes` | `DemoVolumes` |
| Accounts | `AccountsService` | `SystemAccounts` | `DemoAccounts` |
| Terminal | `TerminalService` | `SystemTerminal` | pending |
| Info | `InfoService` (pending) | `SystemInfo` | pending |

## Boundary rule

`anth-panel` may:

- hold `Arc<dyn ...Service>`, call it, and enforce policy/guards;
- read its own config, state, secrets and embedded UI.

`anth-panel` must not:

- call `std::process::Command`;
- read `/proc`, `/sys`, `/etc`, mountinfo or block devices directly;
- open PTYs or send signals directly.

Those operations belong in `anth-host`. The demo build selects `Demo*`
implementations; the real build selects `System*`. A future CI check will grep
for forbidden calls in `anth-panel`.
