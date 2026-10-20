# Account manager: scope and architecture

Status: **proposal**. Nothing below is implemented. Decisions marked
*recommended* need a yes/no before coding starts; the open questions are in §13.

## 1. Why this feature is different

Every other subsystem in the panel reads the host and, at most, toggles a
reversible switch: a signal, an interface, an nft rule, a mount. Account
management is the first feature that can **hand out root**. Create a user, set
its password, add it to `sudo`, and an unauthenticated HTTP port has become a
remote root shell with a persistence mechanism attached.

That fact drives most of the decisions below:

- mutations are off by default, and password management is a second switch;
- a new account is always created **locked** — setting a password is a separate
  operation with its own gate;
- password material never appears in a snapshot, a log line, an error detail or
  a command line;
- the guards refuse to remove the last administrator, so the panel cannot lock
  the operator out of the host it runs on.

`docs/terminal.md` and `docs/firewall.md` already carry a "no authentication
yet" warning. This feature deserves the loudest one.

## 2. Scope

### In scope for v1

- Read-only inventory of users and groups, local and from NSS.
- Create, modify and delete a **local** user: uid, primary group, GECOS, home,
  shell, system flag, lock/unlock.
- Create, modify and delete a **local** group, and replace its member list.
- Set a password, behind a separate switch.
- CLI tables, HTTP routes, Lit component, manual refresh.

### Out of scope for v1

| Deferred | Why |
| --- | --- |
| Changing an existing user's uid/gid | Orphans every file it owns. Needs a chown walk, which is its own feature with its own guards. |
| `authorized_keys` editing | A whole file format plus an injection surface, and it is the single most valuable thing to write to a host. Deserves its own design. |
| sudoers editing | Same class as above; worse, a syntax error can break `sudo` for everyone. Group membership is the supported lever. |
| Last-login and failed-attempt history | `lastlog`/`faillog` are deprecated and frequently absent; `wtmp` parsing is a rabbit hole. |
| Deleting or modifying an NSS-only account | The panel could not write it back even if it wanted to. Refused explicitly, see §6. |
| SSE | Same reason as volumes: no v1 watcher. `poll::stream` makes it cheap later. |

## 3. Data flow

```text
CLI                      HTTP                          Lit component
 |                        |                                 |
 account list             GET  /api/accounts               AccountClient
 account user create      POST /api/accounts/users              |
 account user modify      POST /api/accounts/users/{name}       |
 account user delete      POST /api/accounts/users/{name}/delete|
 account user password    POST /api/accounts/users/{name}/password
 account group ...        POST /api/accounts/groups...          |
 |                        |                                    |
 +------------------------+------------------------------------+
                          |
              anth_proto::Request / Response
                          |
              anth_panel::daemon (dispatch + policy)
                          |
              anth_panel::accounts
               ├── local passwd/group/shadow/gshadow parsers (pure)
               ├── NssReader    (trait; `getent` in prod, text in tests)
               ├── Backend      (trait; shadow-utils / busybox adapters)
               ├── guards       (pure, no I/O)
               └── collect / fit_snapshot
```

The daemon owns the policy, as it does for every other subsystem. The wire
types never carry "control is enabled"; that is deployment state.

## 4. Read model: two sources, one writable

This mirrors the volume manager's device/mount split, for the same reason: one
source cannot answer both questions.

| Source | Answers | Writable |
| --- | --- | --- |
| `/etc/passwd`, `/etc/group`, `/etc/shadow`, `/etc/gshadow` | What the panel can change | Yes |
| `getent passwd`, `getent group` | What the host actually resolves, including LDAP/SSSD/systemd-homed | No |

The snapshot carries a merged list; every entry says where it came from:

```rust
pub enum AccountSource { Local, Nss }
```

An entry present in `getent` but absent from the local files is `Nss`. Every
mutation on an `Nss` entry returns `AccountErrorKind::Unsupported` with a detail
naming the source. Silently writing a local user with a name that NSS already
owns would shadow the directory account and is exactly the kind of subtle
breakage this field exists to prevent.

Reading `getent` rather than only parsing the files is not optional: a host
joined to a domain resolves users the files know nothing about, and an account
list that hides them is a lie.

`locked` is derived from the shadow field: a hash starting with `!` or `*` is
locked. For an `Nss` entry there is no shadow to read, so:

```rust
pub locked: Option<bool>,   // None = unknown, not "unlocked"
```

`None` is a real state, not an absence to be defaulted. The frontend renders it
as `—`. This is the same failure the volume review caught with `mountpoints`:
a client that treats a missing fact as a negative one shows the wrong thing.

## 5. Write model: shell out, behind a trait

Mutations call `useradd`, `usermod`, `userdel`, `groupadd`, `groupmod`,
`groupdel`, `gpasswd` and `chpasswd` through `command.rs`, never a shell string.

Why not write the files directly:

- **Password hashing.** `chpasswd` hashes with whatever the host is configured
  for (`yescrypt` on modern Debian, `sha512` elsewhere). Reimplementing
  `crypt(3)` is not on the table.
- **Locking.** `vipw` and `useradd` take a lock on `/etc/passwd`. A hand-rolled
  writer races them and can lose an entry.
- **The boring parts.** uid allocation, `login.defs` ranges, skel copying, mail
  spool, `subuid`/`subgid`, and the exact shadow field layout differ per distro
  and per tool version.

This is the `nft`/`lsblk` trade-off again, and the `Backend` trait keeps the
logic testable without spawning anything.

### 5.1 The BusyBox problem (verified)

The shipped image is `alpine:3.20`, which ships BusyBox, not shadow-utils. I
checked the applet list against a BusyBox 1.35 binary:

```text
present:  adduser  addgroup  deluser  delgroup  chpasswd  passwd
absent:   useradd  usermod  groupmod  groupadd  gpasswd  chage  getent
```

So on the shipped image as it is today, "modify a user" and "modify a group"
are simply impossible, the shadow-utils flags do not apply, and even the NSS
read has no `getent` — it lives in Alpine's `musl-utils` package, not BusyBox.

Two consequences:

1. `Backend` needs a capability probe, and unsupported operations return
   `AccountErrorKind::Unsupported` rather than guessing with different flags.
   Without `getent` the snapshot degrades to local-only plus a warning.
2. *Recommended:* add `musl-utils` and `shadow` to the runtime stage of the
   `Dockerfile`, which makes one adapter — not two — the shipped path, so the
   matrix that runs in production is the one the tests cover. Two caveats found
   while checking the package index:
   - `shadow` lives in **community**, not `main` (main has only `musl-utils`).
     The image must have that repository enabled.
   - it is 510 KB compressed / 1.4 MB installed and pulls `libbsd` and `libpam`,
     and its tools are PAM-built, so Alpine needs working `/etc/pam.d` entries
     for `chpasswd` and friends. Verify before relying on it; this is the one
     plumbing detail that can make phase 3 fail late.

The alternative is a BusyBox-only adapter, which would mean a second flag matrix
and a feature set (no modify at all) that differs from what the docs promise.
That is why the recommendation is to add the package instead.

### 5.2 The root problem

The container runs `USER anth`. Account management needs root: it writes
`/etc/passwd`, `/etc/shadow` and `/etc/sudoers.d`, and creates home directories
owned by the new uid. `SystemRunner`'s `sudo -n` fallback covers a host where the
daemon has a `NOPASSWD` rule, but the shipped container has none.

So v1 will report `PermissionDenied` with an actionable detail in the default
container. The docs must say so plainly and give the two supported deployments:
run the container as root with the needed capabilities, or install a narrow
sudoers rule for the eight account commands. Do not paper over this.

## 6. Guards

All guards are pure functions over a snapshot plus the policy, tested as a
matrix. Nothing reaches `Backend` before every guard passes.

### 6.1 Always refused, regardless of configuration

These are not configurable. A switch that can be turned on to delete `root` is
not a switch, it is a footgun.

1. **uid 0 and gid 0.** No modify, delete, lock, password change, or group
   membership change.
2. **The daemon's own account.** No delete, lock, or change of uid/gid/shell/
   groups. Demoting itself would take the panel offline mid-request; the
   volume manager has the same rule for its own executable.
3. **Removing the last administrator.** If a mutation would leave `sudo`,
   `wheel` or `admin` with zero members, it is refused. This is the loopback
   guard of this subsystem: the operator must not be able to lock the host.
4. **`Nss` entries**, as described in §4.
5. **Anything the backend reports as unsupported.**

### 6.2 Refused unless the operator opted in

6. **System accounts** (`uid < UID_MIN`, read from `/etc/login.defs`, fallback
   1000) unless `accounts.allow_system`. Protects `daemon`, `bin`, `nobody`,
   `systemd-*`, and the `anth` account itself from an inventory-wide edit.
7. **Privileged groups** unless `accounts.allow_privileged_groups`: `sudo`,
   `wheel`, `admin`, `root`, `docker`, `lxd`. Note that `docker` and `lxd` are
   root-equivalent; listing them here is deliberate.
8. **Removing a home directory** unless `--remove-home` is passed explicitly.
   Never implicit in a delete.

### 6.3 Input validation

Every string reaches an argv vector, so validation is also injection defence.

| Field | Rule |
| --- | --- |
| `name` | `^[a-z_][a-z0-9_-]{0,31}$`. The leading-character rule also stops `-rf` from being parsed as a flag by the backend. |
| `gecos` | No `:`, no newline, no control characters — any of them corrupts `/etc/passwd`. |
| `home` | Absolute, no `..`, and the canonicalised parent must be inside `accounts.home_root`. `useradd` creates it, so the parent is what exists; same shape as `files.root`. |
| `shell` | Must appear in the allowlist: `accounts.shells`, else `/etc/shells`, else a small built-in set. A user must not be able to name an arbitrary binary as a login shell. |
| `uid`, `gid` | Refused when 0. When explicit, refused if the id or the name is already taken. |
| `password` | Never on argv (world-readable in `/proc`); written to `chpasswd` on stdin. |

### 6.4 Confirmation

The CLI asks before `delete`, `password`, and any group membership change,
unless `--yes`. The HTTP routes are confirmation-free by construction — the
frontend's dialog is the equivalent — which is another reason control defaults
to off.

## 7. Wire protocol (`anth-proto/src/accounts.rs`)

```rust
pub enum AccountSource { Local, Nss }

pub struct AccountUser {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub group: String,              // resolved primary group name
    pub gecos: String,
    pub home: String,
    pub shell: String,
    pub locked: Option<bool>,       // None = unknown (Nss)
    pub system: bool,               // uid < UID_MIN
    pub source: AccountSource,
    pub groups: Vec<String>,        // supplementary, always serialized
}

pub struct AccountGroup {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,       // always serialized
    pub system: bool,
    pub source: AccountSource,
}

pub struct AccountSnapshot {
    pub users: Vec<AccountUser>,
    pub groups: Vec<AccountGroup>,
    pub control_enabled: bool,
    pub password_enabled: bool,
    pub timestamp_ms: u64,
    pub total_users: usize,
    pub total_groups: usize,
    pub truncated: bool,
    pub warning: Option<String>,
}
```

No password hash, no shadow field, no `gshadow` secret ever enters this struct.
`groups` and `members` are always serialized, following the volume review
finding: a client must be able to read them as lists without checking for the
key.

Requests:

```rust
AccountList,
AccountCreate { name, uid: Option<u32>, gid: Option<u32>, gecos: Option<String>,
                home: Option<String>, shell: Option<String>, system: bool },
AccountModify { name, gecos: Option<String>, shell: Option<String>,
                home: Option<String>, primary_gid: Option<u32>,
                locked: Option<bool> },
AccountDelete { name, remove_home: bool },
AccountSetPassword { name, password: String },
AccountGroupCreate { name, gid: Option<u32>, system: bool },
AccountGroupModify { name, gid: Option<u32>, members: Option<Vec<String>> },
AccountGroupDelete { name },
```

`AccountCreate` deliberately has **no password field**. A new account is created
locked; unlocking it is `AccountSetPassword`, which has its own configuration
switch and its own audit story.

`AccountGroupModify` replaces the full member list rather than offering
add/remove. Replacement is idempotent, so a retried request cannot partially
apply, and the client already has the list it is editing.

Responses: `AccountSnapshot`, `AccountChanged { message }`,
`AccountError(AccountError)`.

```rust
pub enum AccountErrorKind {
    Disabled, Protected, NotFound, Conflict, Unsupported, Invalid,
    PermissionDenied, Failed,
}
```

`Debug` for `AccountSetPassword` is implemented by hand to print
`password: "<redacted>"`. A derived `Debug` plus one `eprintln!("{request:?}")`
would put a plaintext password in the journal.

## 8. Backend module (`anth-panel/src/accounts.rs`)

```rust
// read path
pub fn collect(control: bool, passwords: bool) -> AccountSnapshot;
pub fn parse_passwd(text: &str) -> Vec<PasswdEntry>;
pub fn parse_group(text: &str) -> Vec<GroupEntry>;
pub fn parse_shadow(text: &str) -> HashMap<String, ShadowEntry>;   // hash used only for "locked"
pub fn read_login_defs(text: &str) -> IdRanges;
fn fit_snapshot(snapshot: AccountSnapshot) -> AccountSnapshot;

// write path
pub fn create_user(policy: &Policy, backend: &dyn Backend, req: &CreateUser)
    -> Result<String, AccountError>;
pub fn create_group(...) -> Result<String, AccountError>;
// ... one entry point per mutation, each: collect_unfitted -> validate -> backend

// guards (pure)
fn validate_create_user(snapshot: &AccountSnapshot, ranges: &IdRanges,
                        policy: &Policy, req: &CreateUser) -> Result<PlannedCall, AccountError>;
fn validate_delete_user(...) -> Result<PlannedCall, AccountError>;
fn validate_group_membership(...) -> Result<PlannedCall, AccountError>;
```

`PlannedCall` is the validated argv + stdin, which is what makes the guard tests
worth writing: a test asserts the exact vector, so "no password in argv" and "no
shell interpolation" are compile-time-checkable properties of the tests, not
prose in a doc.

`collect_unfitted` and `fit_snapshot` are split exactly as in `volumes.rs`, and
for the reason that review established: **mutations validate against the
unfitted inventory**. Truncation is a display concern; a guard that read the
fitted snapshot would refuse to act on the 5000th user of a large host, which is
precisely where the feature is needed.

### 8.1 Failure and degradation

- Missing `/etc/shadow` (unreadable, common for a non-root daemon): `locked`
  becomes `None` for every user and a `warning` explains it. Inventory still
  works.
- Missing `getent`: fall back to the local files and warn.
- Torn reads: `/etc/passwd` is rewritten in place by some tools. Parse
  defensively — a malformed line is skipped and counted, and a warning reports
  how many were skipped rather than failing the whole request. Same rule as the
  unreadable `/proc` fields in `hostinfo.rs`.
- `chpasswd` rejecting a weak password: surface its message verbatim in
  `detail`; do not retry, do not normalise.

## 9. Configuration

```toml
[accounts]
control = false                 # create/modify/delete users and groups
manage_passwords = false        # set passwords; a second switch on purpose
allow_system = false            # touch uid/gid below UID_MIN
allow_privileged_groups = false # touch sudo, wheel, admin, root, docker, lxd
home_root = "/home"             # new homes must resolve below this
shells = []                     # empty = read /etc/shells
default_shell = "/bin/sh"
```

`dev_default()` turns `control` on for debug builds like the other subsystems,
and **leaves `manage_passwords` off** even there. A debug build cannot leak a
credential by accident.

`password_enabled` is reported in the snapshot next to `control_enabled`, so the
UI shows the password action only when it will work.

## 10. Surfaces

### CLI

```text
anth-panel account list [--groups]
anth-panel account user create <NAME> [--uid N] [--group NAME|--gid N]
        [--gecos TEXT] [--home PATH] [--shell PATH] [--system] [--no-create-home]
anth-panel account user modify <NAME> [--gecos TEXT] [--shell PATH] [--home PATH]
        [--group NAME|--gid N] [--lock|--unlock]
anth-panel account user delete <NAME> [--remove-home] [--yes]
anth-panel account user password <NAME> [--stdin] [--yes]
anth-panel account group create <NAME> [--gid N] [--system]
anth-panel account group modify <NAME> [--gid N] [--members a,b,c]
anth-panel account group delete <NAME> [--yes]
```

`user password` prompts twice without echo and refuses a mismatch. `--stdin`
exists for scripting and reads one line; neither path ever puts the secret in
argv or in shell history.

`render_account_snapshot` in `render.rs` draws two bordered tables — users, then
groups — with a blank separator, matching the volume renderer.

### HTTP

| Method | Route | Body |
| --- | --- | --- |
| GET | `/api/accounts` | — |
| POST | `/api/accounts/users` | `{name,uid?,gid?,gecos?,home?,shell?,system?}` |
| POST | `/api/accounts/users/{name}` | `{gecos?,shell?,home?,primary_gid?,locked?}` |
| POST | `/api/accounts/users/{name}/delete` | `{remove_home?}` |
| POST | `/api/accounts/users/{name}/password` | `{password}` |
| POST | `/api/accounts/groups` | `{name,gid?,system?}` |
| POST | `/api/accounts/groups/{name}` | `{gid?,members?}` |
| POST | `/api/accounts/groups/{name}/delete` | — |

Status mapping: `Disabled`/`Protected`/`PermissionDenied` → `403`, `NotFound` →
`404`, `Conflict` → `409`, `Invalid` → `400`, `Unsupported` → `501`, `Failed` →
`500`.

### Frontend

`lib/accounts/account-client.ts`, `lib/accounts/display.ts` (id ranges, system
badges, lock state, sorting), `components/anth-account-manager.ts` with a users
table, a groups table, a create dialog and lock/unlock/delete actions. Controls
render only when `control_enabled`; the password action only when
`password_enabled`; rows whose `source` is `Nss` are read-only and labelled.
Depends only on the injected `AccountClient`, reuses `formatBytes`-style shared
helpers where they apply, and ships an `anth-account-manager.test.html` harness
registered in `component-index.test.html`.

## 11. Tests

- **`anth-proto`**: round-trip for every request and response; `AccountSetPassword`
  `Debug` redaction; `locked`/`source`/`groups`/`members` always serialized.
- **`tests/accounts.rs`**: fixture parsers — passwd/group/shadow/gshadow,
  malformed and truncated lines, duplicate ids, empty GECOS; merged local+NSS
  view; `login.defs` ranges with fallbacks; guard matrix over a `FakeBackend`
  asserting the exact argv, including that **no password ever appears in argv**
  and that every `Nss` mutation is `Unsupported`; last-administrator guard;
  truncation-under-cap asserting a dropped user is still addressable.
- **`tests/api.rs`**: every route through `oneshot`, the `403`-when-disabled
  path, and `409` on a duplicate name.
- **`tests/render.rs`**: alignment with multi-byte GECOS and an empty snapshot.
- **Frontend Vitest**: client URL/body/error mapping, display helpers, component
  with a fake client — control hidden when disabled, password hidden when
  disabled, `Nss` rows read-only, confirmation before delete, no refresh while a
  mutation is in flight.

Two tests are non-negotiable because they cover the failure that matters:
`a_password_never_reaches_argv` and `the_last_administrator_cannot_be_removed`.

## 12. Delivery phases

| Phase | Content | Commit |
| --- | --- | --- |
| 0 | `command.rs`: stdin support for `chpasswd`, plus tests | `refactor: let the command runner feed stdin` |
| 1 | `anth-proto::accounts` types, requests, responses | `feat: add account wire types` |
| 2 | Read-only: parsers, NSS merge, `collect`, `fit_snapshot`, CLI `account list`, renderer, `GET /api/accounts` | `feat: add the account inventory` |
| 3 | Mutations: `Backend` adapters, guards, `[accounts]` config, CLI, HTTP routes | `feat: create, modify and delete accounts` |
| 4 | `Dockerfile`: add `shadow`, document the privilege requirement | `chore: ship shadow-utils in the image` |
| 5 | Frontend client, display helpers, component, harness, index entry | `feat: add the account manager component` |
| 6 | Docs: `README.md`, `cli.md`, `api-endpoints.md`, `socket-protocol.md`, `accounts.md`, `AGENTS.md` | `docs: document account management` |

Phases 1–3 are independently reviewable and each ends green under
`cargo test --workspace`, `cargo fmt --check` and `cargo clippy`.

## 13. Open questions — need a decision before phase 1

1. **Authentication first?** This is the first feature that grants root. The
   honest recommendation is to build the auth layer before this one, or to
   accept that enabling `control` on a reachable interface is equivalent to
   publishing a root password. Which do we do?
2. **Passwords in v1?** Keeping `manage_passwords` as a separate, default-off
   switch is the compromise. The stricter option is to ship v1 with no password
   support at all and require out-of-band `passwd` on the host.
3. **BusyBox or shadow in the image?** Recommended: add `shadow` and drop the
   busybox adapter. Confirming this removes half the adapter matrix.
4. **Changing uid/gid**: defer, as proposed? It orphans file ownership, and the
   fix is a recursive `chown` the panel has no business doing unauthenticated.
5. **Naming**: `account` with `user`/`group` subcommands, matching
   `volume`/`network`. Confirm, since `user` would be the shorter alias.

## 14. Risks

| Risk | Mitigation |
| --- | --- |
| Unauthenticated root grant | `control` and `manage_passwords` default off; documented as the loudest warning in the project |
| Password leaking via logs, argv or snapshot | Never in `Debug`, never in argv, never in the wire snapshot; `chpasswd` on stdin; two dedicated tests |
| Locking the operator out | Last-administrator guard; uid 0 and the daemon's own account always refused |
| Corrupting `/etc/passwd` | Backend tools own the writes and their locking; GECOS validation rejects `:` and newlines |
| `--remove-home` deleting something else | Home validated to canonicalise inside `home_root`, never `/`, never `/home`, never the daemon's home |
| Name injection into argv | Anchored name regex plus a leading-character rule; argv only, never a shell |
| NSS entry shadowed by a local user | `source` field; every `Nss` mutation refused as `Unsupported` |
| Tool drift (BusyBox vs shadow-utils) | Capability probe; unsupported operations fail loudly as `501` rather than guessing flags |
| `shadow` is in Alpine `community`, PAM-built | Enable the repo, verify `/etc/pam.d` before phase 3; `getent` needs `musl-utils` |
| Unprivileged container | Documented `PermissionDenied` with the two supported deployments |
| Large hosts | Same fitted/unfitted split as volumes; mutations read the unfitted inventory |
