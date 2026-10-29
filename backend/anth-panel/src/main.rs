//! `anth-panel` binary: daemon, HTTP API and CLI client.

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::client::Client;
use anth_panel::config::Config;
use anth_panel::dispatch::Daemon;
use anth_panel::lifecycle::{self, Lifecycle};
use anth_panel::log::{Format, Level, Logger, Out, Settings};
use anth_panel::{render, socket};
use anth_wire::{
    FirewallAction, FirewallChain, FirewallProtocol, PROTOCOL_VERSION, PartitionKind, Request,
    Response, parse_interval,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::net::TcpListener;

type CliResult = Result<(), (u8, String)>;

#[derive(Parser, Debug)]
#[command(name = "anth-panel", version, about = "Anthracite Panel")]
struct Cli {
    /// Path to the configuration file.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Log level: error, warn, info, debug or trace.
    #[arg(long, global = true)]
    log_level: Option<String>,

    /// Log format: human, json or off.
    #[arg(long, global = true)]
    log_format: Option<String>,

    /// Log destination: stdout or stderr.
    #[arg(long, global = true)]
    log_out: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Serve the Unix socket and the HTTP API in one process.
    Run {
        /// HTTP bind address.
        #[arg(long, default_value = "0.0.0.0:8080")]
        addr: String,
        /// Let `reboot` take the host down.
        #[arg(long)]
        allow_reboot: bool,
    },
    /// Serve the Unix socket only.
    Daemon {
        /// Let `reboot` take the host down.
        #[arg(long)]
        allow_reboot: bool,
    },
    /// Replace the running daemon with a fresh copy of itself, re-reading the
    /// configuration.
    Restart,
    /// Ask the host to reboot.
    Reboot {
        /// Seconds to wait before the host goes down (at most 300).
        #[arg(long, default_value_t = 0)]
        delay: u32,
    },
    /// Browse and read files under the configured root.
    Files {
        #[command(subcommand)]
        command: FilesCommand,
    },
    /// Open an interactive shell on a pseudo-terminal.
    Terminal {
        /// Initial columns; defaults to the local terminal.
        #[arg(long, value_name = "COLS")]
        cols: Option<u16>,
        /// Initial rows; defaults to the local terminal.
        #[arg(long, value_name = "ROWS")]
        rows: Option<u16>,
    },
    /// Inspect the process table, signal a process or start one.
    Process {
        #[command(subcommand)]
        command: ProcessCommand,
    },
    /// Print a snapshot of the host the daemon runs on.
    HostInfo {
        /// Keep reading every INTERVAL instead of printing once.
        #[arg(
            long,
            value_name = "INTERVAL",
            num_args = 0..=1,
            default_missing_value = anth_wire::DEFAULT_INTERVAL
        )]
        watch: Option<String>,
    },
    /// Inspect network interfaces and optionally change them.
    Network {
        #[command(subcommand)]
        command: NetworkCommand,
    },
    /// Inspect and configure the managed nftables firewall.
    Firewall {
        #[command(subcommand)]
        command: FirewallCommand,
    },
    /// Inspect block devices, mounts and swap areas.
    #[command(alias = "volumes")]
    Storage {
        #[command(subcommand)]
        command: StorageCommand,
    },
    /// Inspect and manage local users and groups.
    #[command(alias = "accounts")]
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Inspect, install and manage signed extension bundles.
    Extension {
        #[command(subcommand)]
        command: ExtensionCommand,
    },
    /// Read, validate or change the configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Plan or write a v0.2 to v0.3 configuration migration.
    Migrate {
        /// v0.2 configuration file.
        #[arg(long, value_name = "FILE", default_value = "config.toml")]
        from: PathBuf,
        /// Write the result; without it the command is a dry run.
        #[arg(long)]
        write: bool,
        /// Overwrite an existing target and secrets file.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ExtensionCommand {
    /// List installed bundles and their state.
    #[command(alias = "ls")]
    List,
    /// Install one signed zip bundle.
    Install {
        /// Path to the bundle.
        bundle: PathBuf,
        /// Accept a changed capability surface.
        #[arg(long)]
        accept_capabilities: bool,
    },
    /// Remove one installed extension and its grant.
    Remove {
        /// Extension id or alias.
        id: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Switch one extension on.
    Enable {
        /// Extension id or alias.
        id: String,
    },
    /// Switch one extension off without removing it.
    Disable {
        /// Extension id or alias.
        id: String,
    },
    /// Rewrite the grant from the manifest currently on disk.
    Approve {
        /// Extension id or alias.
        id: String,
        /// Confirm the capability surface.
        #[arg(long)]
        accept_capabilities: bool,
    },
    /// Send one request through the extension's `anth_cli` entry.
    Call {
        /// Extension id or alias.
        id: String,
        /// JSON request, or `-` to read it from stdin.
        request: String,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Print the configuration file exactly as written.
    Get,
    /// Print the effective configuration with defaults applied.
    Show,
    /// Parse and validate the configuration without writing.
    Validate,
    /// Change one dotted key, preserving comments and key order.
    Set {
        /// Dotted key, e.g. files.write or auth.username.
        key: String,
        /// New value, parsed as a TOML literal.
        value: String,
    },
}

#[derive(Subcommand, Debug)]
enum FilesCommand {
    /// List a directory under the file root.
    List {
        /// Path relative to the file root.
        #[arg(long, default_value = "/", value_name = "PATH")]
        path: String,
        /// Maximum entries in this page (1-1000).
        #[arg(long)]
        limit: Option<u16>,
        /// Opaque cursor from a previous page.
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Print a UTF-8 text file.
    Read {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Byte offset to start at.
        #[arg(long, default_value_t = 0)]
        offset: u64,
        /// Maximum bytes to read.
        #[arg(long)]
        max_bytes: Option<u64>,
    },
    /// Write any file's bytes to stdout, including binary files.
    Download {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
    },
    /// Delete a file under the file root. Requires files.write.
    Delete {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
    },
    /// Write a UTF-8 file under the file root; the content comes from stdin.
    Write {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Fail instead of replacing an existing file.
        #[arg(long)]
        create: bool,
    },
    /// Upload arbitrary bytes from stdin. Requires files.write.
    Upload {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Fail instead of replacing an existing file.
        #[arg(long)]
        create: bool,
    },
    /// Move one entry to the freedesktop trash. Requires files.trash.
    Trash {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
    },
    /// Inspect or empty the trash.
    TrashBin {
        #[command(subcommand)]
        command: TrashBinCommand,
    },
    /// Compress one entry.
    Archive {
        /// Path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Archive path; defaults beside the entry.
        #[arg(long, value_name = "PATH")]
        destination: Option<String>,
        /// zip, tar, tar.gz, tar.xz or 7z.
        #[arg(long, value_name = "FORMAT")]
        format: Option<String>,
    },
    /// List or extract an archive.
    Unarchive {
        /// Archive path relative to the file root.
        #[arg(long, value_name = "PATH")]
        path: String,
        /// Print the members without extracting.
        #[arg(long)]
        list: bool,
        /// Destination directory; defaults beside the archive.
        #[arg(long, value_name = "PATH")]
        into: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ProcessCommand {
    /// List processes, busiest first.
    #[command(alias = "ls")]
    List {
        /// Maximum rows in this page (1-1000).
        #[arg(long)]
        limit: Option<u16>,
        /// Opaque cursor from a previous page.
        #[arg(long)]
        cursor: Option<String>,
        /// Keep refreshing every INTERVAL instead of printing once.
        #[arg(
            long,
            value_name = "INTERVAL",
            num_args = 0..=1,
            default_missing_value = anth_wire::DEFAULT_INTERVAL
        )]
        watch: Option<String>,
    },
    /// Deliver an allowlisted signal to one process.
    Signal {
        /// Process id from `process list`.
        pid: u32,
        /// term, kill, hup, int, quit, stop or cont.
        signal: String,
        /// Signal the process group led by PID.
        #[arg(long)]
        group: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Start a program from `[process] spawn_allow`.
    Spawn {
        /// Absolute path of the program.
        program: String,
        /// Arguments, passed through without a shell.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        /// Absolute working directory.
        #[arg(long)]
        cwd: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum NetworkCommand {
    /// List interfaces, addresses and default routes.
    #[command(alias = "ls")]
    List {
        /// Keep reading every INTERVAL instead of printing once.
        #[arg(
            long,
            value_name = "INTERVAL",
            num_args = 0..=1,
            default_missing_value = anth_wire::DEFAULT_INTERVAL
        )]
        watch: Option<String>,
    },
    /// Bring one interface up.
    Up {
        /// Interface name.
        name: String,
    },
    /// Bring one interface down.
    Down {
        /// Interface name.
        name: String,
    },
    /// Create one virtual interface.
    Create {
        /// Interface name.
        name: String,
        /// veth, bridge, bond, vlan, dummy or tun-tap.
        #[arg(long, value_enum, default_value_t = NetworkKind::Dummy)]
        kind: NetworkKind,
        /// Peer interface, required for a veth.
        #[arg(long)]
        peer: Option<String>,
        /// Parent interface, required for a vlan.
        #[arg(long)]
        link: Option<String>,
        /// VLAN identifier, 1-4094, required for a vlan.
        #[arg(long)]
        vlan_id: Option<u16>,
        /// MTU, 68-65535.
        #[arg(long)]
        mtu: Option<u32>,
        /// Bring the interface up once it exists.
        #[arg(long)]
        up: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Change the MTU or the administrative state of an interface.
    Edit {
        /// Interface name.
        name: String,
        /// MTU, 68-65535.
        #[arg(long)]
        mtu: Option<u32>,
        /// Bring the interface up.
        #[arg(long, conflicts_with = "down")]
        up: bool,
        /// Bring the interface down.
        #[arg(long)]
        down: bool,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Rename an interface that is down.
    Rename {
        /// Current interface name.
        name: String,
        /// New interface name.
        new_name: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Delete an interface.
    Delete {
        /// Interface name.
        name: String,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum NetworkKind {
    Veth,
    Bridge,
    Bond,
    Vlan,
    Dummy,
    #[value(name = "tun-tap")]
    TunTap,
}

impl NetworkKind {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Veth => "veth",
            Self::Bridge => "bridge",
            Self::Bond => "bond",
            Self::Vlan => "vlan",
            Self::Dummy => "dummy",
            Self::TunTap => "tun",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::TunTap => "tun/tap",
            other => other.as_wire(),
        }
    }
}

#[derive(Subcommand, Debug)]
enum FirewallCommand {
    /// Show the managed table and its rules.
    #[command(alias = "list")]
    Status,
    /// Allow traffic to a port by adding an accept rule.
    Allow(FirewallAddArgs),
    /// Deny traffic to a port.
    Deny(FirewallAddArgs),
    /// Edit one managed rule: action, target and comment.
    Edit(FirewallEditArgs),
    /// Change the default policy of the input or output chain.
    Policy(FirewallPolicyArgs),
    /// Delete one rule by its nftables handle.
    Delete(FirewallDeleteArgs),
    /// Remove every rule from one managed chain.
    Flush(FirewallFlushArgs),
}

#[derive(Clone, Copy, Debug)]
struct PortSpec {
    port: u16,
    protocol: FirewallProtocol,
}

impl std::str::FromStr for PortSpec {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (port, protocol) = match value.split_once('/') {
            Some((port, protocol)) => (port, protocol.parse()?),
            None => (value, FirewallProtocol::Tcp),
        };
        let port = port
            .parse::<u16>()
            .map_err(|_| format!("invalid port `{port}`; expected a number between 1 and 65535"))?;
        if port == 0 {
            return Err("port must be between 1 and 65535".to_string());
        }
        Ok(Self { port, protocol })
    }
}

#[derive(Args, Debug)]
struct FirewallAddArgs {
    /// Port to allow or deny, optionally suffixed with `/tcp` or `/udp`.
    spec: PortSpec,
    /// Chain to manage: input or output.
    #[arg(long, default_value = "input")]
    chain: FirewallChain,
    /// Optional comment: letters, digits, dot, underscore and dash.
    #[arg(long)]
    comment: Option<String>,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct FirewallEditArgs {
    /// Rule handle shown by `firewall status`.
    handle: u64,
    /// New port, optionally suffixed with `/tcp` or `/udp`. Defaults to the current target.
    spec: Option<PortSpec>,
    /// Chain to move the rule to. Defaults to the current chain.
    #[arg(long)]
    chain: Option<FirewallChain>,
    /// New action: allow or deny. Defaults to the current action.
    #[arg(long)]
    action: Option<String>,
    /// New comment; an empty string clears it. Defaults to the current comment.
    #[arg(long)]
    comment: Option<String>,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct FirewallPolicyArgs {
    /// Chain whose default policy changes: input or output.
    chain: FirewallChain,
    /// New policy: allow (accept) or deny (drop).
    policy: FirewallAction,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct FirewallDeleteArgs {
    /// Rule handle shown by `firewall status`.
    handle: u64,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct FirewallFlushArgs {
    /// Chain to empty: input or output.
    #[arg(long, default_value = "input")]
    chain: FirewallChain,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Subcommand, Debug)]
enum AccountCommand {
    /// List users and groups.
    #[command(alias = "ls")]
    List,
    /// Create one locked local user.
    Create(AccountCreateArgs),
    /// Delete one local user.
    Delete(AccountDeleteArgs),
    /// Set one local user's password from stdin.
    Password(AccountPasswordArgs),
    /// Create or delete a local group.
    Group {
        #[command(subcommand)]
        command: AccountGroupCommand,
    },
}

#[derive(Subcommand, Debug)]
enum AccountGroupCommand {
    /// Create one local group.
    Create(AccountGroupCreateArgs),
    /// Delete one local group.
    Delete(AccountGroupDeleteArgs),
}

#[derive(Args, Debug)]
struct AccountCreateArgs {
    /// Login name, lowercase letters, digits, `_` and `-`.
    name: String,
    /// Explicit uid; defaults to the host's range.
    #[arg(long)]
    uid: Option<u32>,
    /// Existing primary gid.
    #[arg(long)]
    gid: Option<u32>,
    /// Description (GECOS) without `:` or newlines.
    #[arg(long)]
    gecos: Option<String>,
    /// Home inside `accounts.home_root`; defaults to `<home_root>/<name>`.
    #[arg(long)]
    home: Option<String>,
    /// Login shell from the allowlist; defaults to `accounts.default_shell`.
    #[arg(long)]
    shell: Option<String>,
    /// Create a system account; requires `accounts.allow_system`.
    #[arg(long)]
    system: bool,
}

#[derive(Args, Debug)]
struct AccountDeleteArgs {
    /// Login name.
    name: String,
    /// Also delete the home directory inside `accounts.home_root`.
    #[arg(long)]
    remove_home: bool,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct AccountPasswordArgs {
    /// Login name.
    name: String,
}

#[derive(Args, Debug)]
struct AccountGroupCreateArgs {
    /// Group name.
    name: String,
    /// Explicit gid; defaults to the host's range.
    #[arg(long)]
    gid: Option<u32>,
    /// Create a system group; requires `accounts.allow_system`.
    #[arg(long)]
    system: bool,
}

#[derive(Args, Debug)]
struct AccountGroupDeleteArgs {
    /// Group name.
    name: String,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Subcommand, Debug)]
enum StorageCommand {
    /// List block devices, mounts and swap areas.
    #[command(alias = "ls")]
    List,
    /// Mount one block device under the configured mount root.
    Mount(StorageMountArgs),
    /// Unmount one current mountpoint.
    Unmount(StorageUnmountArgs),
    /// Create, resize or delete a partition.
    Partition {
        #[command(subcommand)]
        command: StoragePartitionCommand,
    },
    /// Format one partition with a filesystem.
    Format(StorageFormatArgs),
}

#[derive(Subcommand, Debug)]
enum StoragePartitionCommand {
    /// Create one partition on a whole disk.
    Create(StoragePartitionCreateArgs),
    /// Resize one existing partition.
    Resize(StoragePartitionResizeArgs),
    /// Delete one existing partition.
    Delete(StoragePartitionDeleteArgs),
}

#[derive(Args, Debug)]
struct StoragePartitionCreateArgs {
    /// Whole disk, for example /dev/sdb.
    disk: String,
    /// Partition kind.
    #[arg(long, value_enum, default_value_t = PartitionKindArg::Linux)]
    kind: PartitionKindArg,
    /// Size in bytes; must be a multiple of 512.
    #[arg(long)]
    size_bytes: u64,
    /// Offset in bytes; 0 lets sfdisk choose.
    #[arg(long, default_value_t = 0)]
    start_bytes: u64,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct StoragePartitionResizeArgs {
    /// Whole disk, for example /dev/sdb.
    disk: String,
    /// Partition number.
    number: u32,
    /// New size in bytes; must be a multiple of 512.
    #[arg(long)]
    size_bytes: u64,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct StoragePartitionDeleteArgs {
    /// Whole disk, for example /dev/sdb.
    disk: String,
    /// Partition number.
    number: u32,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct StorageFormatArgs {
    /// Partition path, for example /dev/sdb1.
    partition: String,
    /// ext4, xfs, vfat, exfat, btrfs or swap.
    #[arg(long = "type", value_name = "FS")]
    fs_type: String,
    /// Optional filesystem label.
    #[arg(long)]
    label: Option<String>,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum PartitionKindArg {
    Linux,
    Swap,
    Efi,
}

impl PartitionKindArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Swap => "swap",
            Self::Efi => "efi",
        }
    }
}

impl From<PartitionKindArg> for PartitionKind {
    fn from(value: PartitionKindArg) -> Self {
        match value {
            PartitionKindArg::Linux => Self::Linux,
            PartitionKindArg::Swap => Self::Swap,
            PartitionKindArg::Efi => Self::Efi,
        }
    }
}

#[derive(Args, Debug)]
struct StorageMountArgs {
    /// Device path or kernel name, for example /dev/sdb1 or sdb1.
    device: String,
    /// Absolute target directory under the configured mount root.
    target: String,
    /// Filesystem type; defaults to the type lsblk detected.
    #[arg(long = "type", value_name = "FS")]
    fs_type: Option<String>,
    /// Mount read-only.
    #[arg(long)]
    read_only: bool,
    /// Allow a removable device.
    #[arg(long)]
    allow_removable: bool,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Args, Debug)]
struct StorageUnmountArgs {
    /// Mountpoint to unmount.
    target: String,
    /// Do not ask for confirmation.
    #[arg(long)]
    yes: bool,
}

#[derive(Subcommand, Debug)]
enum TrashBinCommand {
    /// List every trashed entry.
    List,
    /// Put one entry back at its recorded path.
    Restore {
        /// Identifier from `trash-bin list`.
        id: String,
        /// Replace an existing target.
        #[arg(long)]
        overwrite: bool,
    },
    /// Delete everything the trash holds.
    Empty,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            if !message.is_empty() {
                eprintln!("{message}");
            }
            ExitCode::from(code)
        }
    }
}

async fn run() -> CliResult {
    let cli = Cli::parse();
    let path = cli
        .config
        .clone()
        .unwrap_or_else(Config::default_config_path);

    match &cli.command {
        Command::Config { command } => config_command(&path, command, &cli),
        Command::HostInfo { watch } => host_info(&path, watch.clone()).await,
        Command::Network { command } => network_command(&path, command).await,
        Command::Firewall { command } => firewall_command(&path, command).await,
        Command::Storage { command } => storage_command(&path, command).await,
        Command::Account { command } => account_command(&path, command).await,
        Command::Extension { command } => extension_command(&path, command),
        Command::Migrate { from, write, force } => migrate_command(&path, from, *write, *force),
        Command::Daemon { allow_reboot } => serve(&path, None, *allow_reboot, &cli).await,
        Command::Restart => lifecycle_command(&path, LifecycleRequest::Restart).await,
        Command::Reboot { delay } => {
            lifecycle_command(&path, LifecycleRequest::Reboot(*delay)).await
        }
        Command::Files { command } => files_command(&path, command).await,
        Command::Terminal { cols, rows } => anth_panel::terminal_client::run(&path, *cols, *rows)
            .await
            .map_err(|code| (code, String::new())),
        Command::Process { command } => process_command(&path, command).await,
        Command::Run { addr, allow_reboot } => {
            serve(&path, Some(addr.clone()), *allow_reboot, &cli).await
        }
    }
}

fn extension_command(path: &std::path::Path, command: &ExtensionCommand) -> CliResult {
    let config = Config::load_or_create(path).map_err(|error| (1, error))?;
    let paths = config.paths(path).map_err(|error| (1, error))?;
    let service = anth_panel::extensions::ExtensionService::new(
        config.extensions.clone(),
        paths.extensions,
        paths.grants,
        std::sync::Arc::new(anth_host::exec::SystemExecRunner::unavailable()),
    );
    match command {
        ExtensionCommand::List => {
            let view = service
                .snapshot()
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            println!("{}", render::extensions(&view));
            Ok(())
        }
        ExtensionCommand::Install {
            bundle,
            accept_capabilities,
        } => {
            let installed = service
                .install_file(bundle, *accept_capabilities)
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            println!(
                "installed {} {} ({} capabilities)",
                installed.grant.id,
                installed.grant.extension_version,
                installed.grant.allowed_ids().len()
            );
            for capability in installed.grant.allowed_ids() {
                println!("  {capability}");
            }
            if let Some(warning) = installed.warning {
                eprintln!("warning: {warning}");
            }
            Ok(())
        }
        ExtensionCommand::Remove { id, yes } => {
            let id = resolve_extension_id(&service, id)?;
            if !yes {
                confirm(&format!("Remove extension {id} and its grant?"))?;
            }
            service
                .remove(&id)
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            println!("removed {id}");
            Ok(())
        }
        ExtensionCommand::Enable { id } | ExtensionCommand::Disable { id } => {
            let id = resolve_extension_id(&service, id)?;
            let enable = matches!(command, ExtensionCommand::Enable { .. });
            service
                .set_enabled(&id, enable)
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            println!("{} is {}", id, if enable { "enabled" } else { "disabled" });
            Ok(())
        }
        ExtensionCommand::Approve {
            id,
            accept_capabilities,
        } => {
            let id = resolve_extension_id(&service, id)?;
            let grant = service
                .approve(&id, *accept_capabilities)
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            println!(
                "approved {} {} ({} capabilities)",
                grant.id,
                grant.extension_version,
                grant.allowed_ids().len()
            );
            for capability in grant.allowed_ids() {
                println!("  {capability}");
            }
            Ok(())
        }
        ExtensionCommand::Call { id, request } => {
            let id = resolve_extension_id(&service, id)?;
            let request = if request == "-" {
                let mut text = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
                    .map_err(|error| (1, format!("reading the request: {error}")))?;
                text
            } else {
                request.clone()
            };
            serde_json::from_str::<serde_json::Value>(&request)
                .map_err(|error| (2, format!("the request is not valid JSON: {error}")))?;
            let logger = std::sync::Arc::new(Logger::with_out(Settings::default()));
            let reply = service
                .call_cli(&id, &request, logger)
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            println!(
                "{}",
                serde_json::json!({ "status": reply.status, "value": reply.value })
            );
            if reply.status >= 400 {
                Err((1, String::new()))
            } else {
                Ok(())
            }
        }
    }
}

fn resolve_extension_id(
    service: &anth_panel::extensions::ExtensionService,
    id_or_alias: &str,
) -> Result<String, (u8, String)> {
    match service.grants().read(id_or_alias) {
        Ok(Some(_)) => return Ok(id_or_alias.to_string()),
        Ok(None) => {}
        Err(error) if error.kind == anth_wire::ErrorKind::Invalid => {
            // Not a valid id, so it can only be an alias. Fall through to the
            // catalogue lookup rather than reporting the shape as an error.
        }
        Err(error) => return Err((error.kind.exit_code(), error.to_string())),
    }
    let view = service
        .snapshot()
        .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
    let mut matches = view
        .extensions
        .iter()
        .filter(|extension| extension.alias.as_deref() == Some(id_or_alias));
    let Some(first) = matches.next() else {
        return Err((1, format!("no extension `{id_or_alias}` is installed")));
    };
    if matches.next().is_some() {
        return Err((
            1,
            format!("more than one extension declares the alias `{id_or_alias}`; use the full id"),
        ));
    }
    Ok(first.id.clone())
}

fn migrate_command(
    target: &std::path::Path,
    source: &std::path::Path,
    write: bool,
    force: bool,
) -> CliResult {
    let text = std::fs::read_to_string(source)
        .map_err(|error| (1, format!("reading {}: {error}", source.display())))?;
    let plan = anth_panel::migrate::plan(&text).map_err(|error| (2, error))?;
    for warning in &plan.warnings {
        eprintln!("warning: {warning}");
    }
    if !write {
        print!("{}", anth_panel::migrate::render(&plan));
        eprintln!(
            "dry run: nothing was written; pass --write to create {}",
            target.display()
        );
        return Ok(());
    }
    let written = anth_panel::migrate::write(&plan, target, force).map_err(|error| (1, error))?;
    println!("wrote {}", written.config.display());
    if let Some(secrets) = written.secrets {
        println!("wrote {} (0600)", secrets.display());
    }
    Ok(())
}

fn config_command(path: &std::path::Path, command: &ConfigCommand, cli: &Cli) -> CliResult {
    let (settings, problems) = client_settings(None, cli);
    for problem in problems {
        eprintln!("{problem}");
    }
    let _logger = Logger::with_out(settings);
    match command {
        ConfigCommand::Get => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| (1, format!("reading {}: {error}", path.display())))?;
            print!("{text}");
            Ok(())
        }
        ConfigCommand::Show => {
            let config = Config::load(path).map_err(|error| (1, error))?;
            print!(
                "{}",
                toml::to_string_pretty(&config)
                    .map_err(|error| (1, format!("rendering the configuration: {error}")))?
            );
            Ok(())
        }
        ConfigCommand::Validate => {
            Config::load(path).map_err(|error| (1, error))?;
            println!("{} is valid", path.display());
            Ok(())
        }
        ConfigCommand::Set { key, value } => {
            Config::set_key(path, key, value).map_err(|error| (2, error))?;
            println!("{} updated", path.display());
            Ok(())
        }
    }
}

async fn serve(
    path: &std::path::Path,
    addr: Option<String>,
    allow_reboot: bool,
    cli: &Cli,
) -> CliResult {
    let config = Config::load_or_create(path).map_err(|error| (1, error))?;
    let paths = config.paths(path).map_err(|error| (1, error))?;
    prepare_state_dir(&paths.state_dir)?;

    let (settings, problems) = client_settings(Some(&config), cli);
    for problem in problems {
        eprintln!("{problem}");
    }
    let logger = Arc::new(Logger::with_out(settings));
    let (auth, generated) = AuthService::load(&config.auth, &paths)
        .map_err(|error| (1, format!("authentication: {error}")))?;
    if let Some(password) = generated {
        eprintln!(
            "\nPanel authentication is now enabled.\nUsername: {}\nPassword: {password}\nThis password is shown once and stored only as a hash.\n",
            config.auth.username
        );
    } else if auth.state().bootstrap == anth_panel::auth::Bootstrap::Web {
        eprintln!(
            "Panel registration is open. POST /api/auth/register or use the web UI to set the first password."
        );
    }

    let daemon = Arc::new(Daemon::with_lifecycle(
        config.clone(),
        paths.clone(),
        Arc::clone(&logger),
        Arc::new(Lifecycle::new(Arc::new(lifecycle::System), allow_reboot)),
    ));
    daemon.spawn_terminal_reaper();
    let socket_listener =
        socket::bind(&paths.socket).map_err(|error| (1, format!("socket: {error}")))?;

    if let Some(addr) = addr {
        let http_listener = TcpListener::bind(&addr)
            .await
            .map_err(|error| (1, format!("binding HTTP {addr}: {error}")))?;
        let state = AppState::new(Arc::clone(&daemon), auth, logger);
        let router = api::router(state);
        eprintln!(
            "anth-panel {} (wire {}) listening on http://{addr} and {}",
            env!("CARGO_PKG_VERSION"),
            PROTOCOL_VERSION,
            paths.socket.display()
        );
        let http = axum::serve(
            http_listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        );
        let socket = socket::serve(socket_listener, Arc::clone(&daemon));
        tokio::select! {
            result = http => result.map_err(|error| (1, format!("HTTP server: {error}"))),
            result = socket => result.map_err(|error| (1, format!("socket server: {error}"))),
        }
    } else {
        eprintln!(
            "anth-panel {} (wire {}) listening on {}",
            env!("CARGO_PKG_VERSION"),
            PROTOCOL_VERSION,
            paths.socket.display()
        );
        socket::serve(socket_listener, daemon)
            .await
            .map_err(|error| (1, format!("socket server: {error}")))
    }
}

enum LifecycleRequest {
    Restart,
    Reboot(u32),
}

/// Opens the socket and performs the handshake; every client command starts here.
async fn connect(path: &std::path::Path) -> Result<Client, (u8, String)> {
    let config = Config::load_or_default(path);
    let paths = config.paths(path).map_err(|error| (1, error))?;
    let mut client = Client::connect(&paths.socket)
        .await
        .map_err(|error| (1, error.to_string()))?;
    client
        .hello()
        .await
        .map_err(|error| (1, error.to_string()))?;
    Ok(client)
}

async fn lifecycle_command(path: &std::path::Path, command: LifecycleRequest) -> CliResult {
    let mut client = connect(path).await?;

    let request = match command {
        LifecycleRequest::Restart => Request::Restart,
        LifecycleRequest::Reboot(delay) => Request::SystemReboot { delay_s: delay },
    };
    let response = client
        .call(&request)
        .await
        .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
    match response {
        Response::Restarting { pid } => {
            println!("restarting the panel (pid {pid}); it re-reads the configuration");
            Ok(())
        }
        Response::Rebooting { delay_s } => {
            if delay_s == 0 {
                println!("the host is rebooting");
            } else {
                println!("the host reboots in {delay_s}s");
            }
            Ok(())
        }
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

async fn files_command(path: &std::path::Path, command: &FilesCommand) -> CliResult {
    let mut client = connect(path).await?;

    match command {
        FilesCommand::List {
            path,
            limit,
            cursor,
        } => {
            let response = client
                .call(&Request::FilesList {
                    path: path.clone(),
                    limit: *limit,
                    cursor: cursor.clone(),
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesList(page) => {
                    print!("{}", render::file_list(&page));
                    println!();
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Read {
            path,
            offset,
            max_bytes,
        } => {
            let response = client
                .call(&Request::FilesRead {
                    path: path.clone(),
                    offset: *offset,
                    max_bytes: *max_bytes,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesRead(chunk) => {
                    print!("{}", chunk.text);
                    if !chunk.text.ends_with('\n') {
                        println!();
                    }
                    if !chunk.eof {
                        eprintln!(
                            "[more at offset {}]",
                            chunk.offset + u64::try_from(chunk.text.len()).unwrap_or(u64::MAX)
                        );
                    }
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Download { path } => {
            use std::io::Write as _;

            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let mut offset = 0u64;
            loop {
                let response = client
                    .call(&Request::FilesDownload {
                        path: path.clone(),
                        offset,
                        max_bytes: None,
                    })
                    .await
                    .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
                let chunk = match response {
                    Response::FilesDownload(chunk) => chunk,
                    Response::Error(error) => {
                        return Err((error.kind.exit_code(), error.to_string()));
                    }
                    other => return Err((1, format!("unexpected response: {other:?}"))),
                };
                let bytes = STANDARD
                    .decode(chunk.data_base64.as_bytes())
                    .map_err(|error| (1, format!("the download chunk is not base64: {error}")))?;
                out.write_all(&bytes)
                    .map_err(|error| (1, format!("writing the download: {error}")))?;
                if chunk.eof {
                    break;
                }
                if bytes.is_empty() {
                    return Err((1, "the download stopped before EOF".to_string()));
                }
                offset = chunk.offset + u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            }
            out.flush()
                .map_err(|error| (1, format!("flushing the download: {error}")))?;
            Ok(())
        }
        FilesCommand::Delete { path } => {
            let response = client
                .call(&Request::FilesDelete { path: path.clone() })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesDeleted { path } => {
                    println!("deleted {path}");
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Write { path, create } => {
            let mut content = String::new();
            std::io::stdin()
                .read_to_string(&mut content)
                .map_err(|error| (1, format!("reading stdin: {error}")))?;
            let response = client
                .call(&Request::FilesWrite {
                    path: path.clone(),
                    content,
                    create: *create,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesWritten(written) => {
                    println!("wrote {} ({} bytes)", written.path, written.size);
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Upload { path, create } => {
            let mut content = Vec::new();
            std::io::stdin()
                .read_to_end(&mut content)
                .map_err(|error| (1, format!("reading stdin: {error}")))?;
            let response = client
                .call(&Request::FilesUpload {
                    path: path.clone(),
                    content_base64: STANDARD.encode(&content),
                    create: *create,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesWritten(written) => {
                    println!("wrote {} ({} bytes)", written.path, written.size);
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Trash { path } => {
            let response = client
                .call(&Request::FilesTrash { path: path.clone() })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesTrashed { path, id } => {
                    println!("trashed {path} as {id}");
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::TrashBin { command } => match command {
            TrashBinCommand::List => {
                let response = client
                    .call(&Request::FilesTrashList)
                    .await
                    .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
                match response {
                    Response::FilesTrashList(listing) => {
                        println!(
                            "trash: {} entries, {} used",
                            listing.total, listing.used_bytes
                        );
                        for entry in &listing.entries {
                            println!(
                                "{}\t{}\t{}",
                                entry.id,
                                entry.original_path,
                                entry.deleted_at.as_deref().unwrap_or("-")
                            );
                        }
                        Ok(())
                    }
                    other => Err((1, format!("unexpected response: {other:?}"))),
                }
            }
            TrashBinCommand::Restore { id, overwrite } => {
                let response = client
                    .call(&Request::FilesTrashRestore {
                        id: id.clone(),
                        overwrite: *overwrite,
                    })
                    .await
                    .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
                match response {
                    Response::FilesRestored { id, path } => {
                        println!("restored {id} to {path}");
                        Ok(())
                    }
                    other => Err((1, format!("unexpected response: {other:?}"))),
                }
            }
            TrashBinCommand::Empty => {
                let response = client
                    .call(&Request::FilesTrashEmpty)
                    .await
                    .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
                match response {
                    Response::FilesTrashEmptied { removed } => {
                        println!("removed {removed} trashed entries");
                        Ok(())
                    }
                    other => Err((1, format!("unexpected response: {other:?}"))),
                }
            }
        },
        FilesCommand::Archive {
            path,
            destination,
            format,
        } => {
            let format = format.as_deref().map(parse_archive_format).transpose()?;
            let response = client
                .call(&Request::FilesArchive {
                    path: path.clone(),
                    destination: destination.clone(),
                    format,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::FilesArchiveCreated { path } => {
                    println!("created {path}");
                    Ok(())
                }
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        FilesCommand::Unarchive { path, list, into } => {
            if *list {
                let response = client
                    .call(&Request::FilesArchiveList { path: path.clone() })
                    .await
                    .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
                match response {
                    Response::FilesArchiveList(listing) => {
                        if let Some(format) = listing.format {
                            println!("{} ({})", listing.path, format.label());
                        }
                        for entry in &listing.entries {
                            println!("{}\t{}", entry.name, entry.size);
                        }
                        if let Some(warning) = listing.warning {
                            eprintln!("{warning}");
                        }
                        Ok(())
                    }
                    other => Err((1, format!("unexpected response: {other:?}"))),
                }
            } else {
                let response = client
                    .call(&Request::FilesArchiveExtract {
                        path: path.clone(),
                        into: into.clone(),
                    })
                    .await
                    .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
                match response {
                    Response::FilesArchiveExtracted { path, entries } => {
                        println!("extracted {path} ({} entries)", entries.len());
                        Ok(())
                    }
                    other => Err((1, format!("unexpected response: {other:?}"))),
                }
            }
        }
    }
}

fn parse_archive_format(value: &str) -> Result<anth_wire::ArchiveFormat, (u8, String)> {
    use anth_wire::ArchiveFormat;
    match value {
        "zip" => Ok(ArchiveFormat::Zip),
        "tar" => Ok(ArchiveFormat::Tar),
        "tar.gz" | "tgz" => Ok(ArchiveFormat::TarGz),
        "tar.xz" | "txz" => Ok(ArchiveFormat::TarXz),
        "7z" => Ok(ArchiveFormat::SevenZ),
        other => Err((
            2,
            format!("unknown archive format `{other}`; use zip, tar, tar.gz, tar.xz or 7z"),
        )),
    }
}

async fn host_info(path: &std::path::Path, watch: Option<String>) -> CliResult {
    let mut client = connect(path).await?;

    let Some(interval) = watch else {
        let response = client
            .call(&Request::HostInfo)
            .await
            .map_err(|error| (1, error.to_string()))?;
        print_host_info(&response)?;
        return Ok(());
    };

    watch_loop(&mut client, &interval, &Request::HostInfo, render_response).await
}

fn print_host_info(response: &Response) -> CliResult {
    println!("{}", render_response(response)?);
    Ok(())
}

async fn network_command(path: &std::path::Path, command: &NetworkCommand) -> CliResult {
    let mut client = connect(path).await?;

    match command {
        NetworkCommand::List { watch } => {
            let Some(interval) = watch else {
                let response = client
                    .call(&Request::Network)
                    .await
                    .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
                println!("{}", render_network(&response)?);
                return Ok(());
            };
            watch_loop(&mut client, interval, &Request::Network, render_network).await
        }
        NetworkCommand::Up { name } => {
            network_change(
                &mut client,
                Request::NetworkSetState {
                    name: name.clone(),
                    up: true,
                },
            )
            .await
        }
        NetworkCommand::Down { name } => {
            network_change(
                &mut client,
                Request::NetworkSetState {
                    name: name.clone(),
                    up: false,
                },
            )
            .await
        }
        NetworkCommand::Create {
            name,
            kind,
            peer,
            link,
            vlan_id,
            mtu,
            up,
            yes,
        } => {
            if !*yes {
                confirm(&format!("Create {} interface `{name}`?", kind.label()))?;
            }
            network_change(
                &mut client,
                Request::NetworkCreate(anth_wire::NetworkCreate {
                    name: name.clone(),
                    kind: kind.as_wire().to_string(),
                    peer: peer.clone(),
                    link: link.clone(),
                    vlan_id: *vlan_id,
                    mtu: *mtu,
                    up: *up,
                }),
            )
            .await
        }
        NetworkCommand::Edit {
            name,
            mtu,
            up,
            down,
            yes,
        } => {
            if mtu.is_none() && !*up && !*down {
                return Err((
                    2,
                    "nothing to change: pass --mtu, --up or --down".to_string(),
                ));
            }
            if !*yes {
                let change = match (mtu, up, down) {
                    (Some(mtu), true, _) => format!("set the MTU to {mtu} and bring it up"),
                    (Some(mtu), _, true) => format!("set the MTU to {mtu} and bring it down"),
                    (Some(mtu), _, _) => format!("set the MTU to {mtu}"),
                    (None, true, _) => "bring it up".to_string(),
                    (None, _, true) => "bring it down".to_string(),
                    (None, false, false) => "change nothing".to_string(),
                };
                confirm(&format!("{change} on interface `{name}`?"))?;
            }
            network_change(
                &mut client,
                Request::NetworkEdit(anth_wire::NetworkEdit {
                    name: name.clone(),
                    mtu: *mtu,
                    up: if *up {
                        Some(true)
                    } else if *down {
                        Some(false)
                    } else {
                        None
                    },
                }),
            )
            .await
        }
        NetworkCommand::Rename {
            name,
            new_name,
            yes,
        } => {
            if !*yes {
                confirm(&format!("Rename `{name}` to `{new_name}`?"))?;
            }
            network_change(
                &mut client,
                Request::NetworkRename {
                    name: name.clone(),
                    new_name: new_name.clone(),
                },
            )
            .await
        }
        NetworkCommand::Delete { name, yes } => {
            if !*yes {
                confirm(&format!(
                    "Delete interface `{name}`? This cannot be undone and may disconnect the host."
                ))?;
            }
            network_change(
                &mut client,
                Request::NetworkDelete {
                    name: name.clone(),
                    peer: None,
                },
            )
            .await
        }
    }
}

async fn network_change(client: &mut Client, request: Request) -> CliResult {
    let response = client
        .call(&request)
        .await
        .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
    match response {
        Response::NetworkChanged { message } => {
            println!("{message}");
            Ok(())
        }
        Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

fn render_network(response: &Response) -> Result<String, (u8, String)> {
    match response {
        Response::Network(snapshot) => Ok(render::network(snapshot)),
        Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

async fn firewall_command(path: &std::path::Path, command: &FirewallCommand) -> CliResult {
    let mut client = connect(path).await?;

    match command {
        FirewallCommand::Status => {
            let response = client
                .call(&Request::FirewallStatus)
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            println!("{}", render_firewall(&response)?);
            Ok(())
        }
        FirewallCommand::Allow(args) => {
            if !args.yes {
                confirm(&format!(
                    "Allow {}/{} traffic on {}?",
                    args.spec.protocol, args.spec.port, args.chain
                ))?;
            }
            firewall_change(
                &mut client,
                Request::FirewallAdd {
                    chain: args.chain,
                    action: FirewallAction::Allow,
                    protocol: args.spec.protocol,
                    port: args.spec.port,
                    comment: args.comment.clone(),
                },
            )
            .await
        }
        FirewallCommand::Deny(args) => {
            if !args.yes {
                confirm(&format!(
                    "Deny {}/{} traffic on {}?",
                    args.spec.protocol, args.spec.port, args.chain
                ))?;
            }
            firewall_change(
                &mut client,
                Request::FirewallAdd {
                    chain: args.chain,
                    action: FirewallAction::Deny,
                    protocol: args.spec.protocol,
                    port: args.spec.port,
                    comment: args.comment.clone(),
                },
            )
            .await
        }
        FirewallCommand::Edit(args) => {
            let snapshot = match client
                .call(&Request::FirewallStatus)
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?
            {
                Response::FirewallSnapshot(snapshot) => snapshot,
                Response::Error(error) => return Err((error.kind.exit_code(), error.to_string())),
                other => return Err((1, format!("unexpected response: {other:?}"))),
            };
            let rule = snapshot
                .rules
                .iter()
                .find(|rule| rule.handle == args.handle)
                .ok_or_else(|| (1, format!("no firewall rule with handle {}", args.handle)))?;
            if rule.action == "custom" {
                return Err((2, "custom rules can be deleted but not edited".to_string()));
            }
            let action = match &args.action {
                Some(value) => value
                    .parse::<FirewallAction>()
                    .map_err(|message| (2, message))?,
                None => rule
                    .action
                    .parse::<FirewallAction>()
                    .map_err(|_| (1, format!("rule #{} has no editable action", args.handle)))?,
            };
            let (protocol, port) = match args.spec {
                Some(spec) => (spec.protocol, spec.port),
                None => (
                    rule.protocol.ok_or_else(|| {
                        (1, format!("rule #{} has no editable protocol", args.handle))
                    })?,
                    rule.port.ok_or_else(|| {
                        (1, format!("rule #{} has no editable port", args.handle))
                    })?,
                ),
            };
            let comment = match &args.comment {
                Some(value) => Some(value.clone()).filter(|text| !text.trim().is_empty()),
                None => rule.comment.clone(),
            };
            let chain = args.chain.unwrap_or(rule.chain);
            if !args.yes {
                confirm(&format!(
                    "Update firewall rule {} on {chain} to {action} {protocol}/{port}?",
                    args.handle
                ))?;
            }
            firewall_change(
                &mut client,
                Request::FirewallUpdate {
                    handle: args.handle,
                    chain,
                    action,
                    protocol,
                    port,
                    comment,
                },
            )
            .await
        }
        FirewallCommand::Policy(args) => {
            if !args.yes {
                let warning = if args.policy == FirewallAction::Deny {
                    " Unmatched traffic will be dropped, which can lock you out."
                } else {
                    ""
                };
                confirm(&format!(
                    "Set the {} policy to {}?{warning}",
                    args.chain, args.policy
                ))?;
            }
            firewall_change(
                &mut client,
                Request::FirewallSetPolicy {
                    chain: args.chain,
                    policy: args.policy,
                },
            )
            .await
        }
        FirewallCommand::Delete(args) => {
            if !args.yes {
                confirm(&format!("Delete firewall rule {}?", args.handle))?;
            }
            firewall_change(
                &mut client,
                Request::FirewallDelete {
                    handle: args.handle,
                },
            )
            .await
        }
        FirewallCommand::Flush(args) => {
            if !args.yes {
                confirm(&format!(
                    "Flush every rule from the {} chain of the managed firewall?",
                    args.chain
                ))?;
            }
            firewall_change(&mut client, Request::FirewallFlush { chain: args.chain }).await
        }
    }
}

async fn firewall_change(client: &mut Client, request: Request) -> CliResult {
    let response = client
        .call(&request)
        .await
        .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
    match response {
        Response::FirewallChanged { message } => {
            println!("{message}");
            Ok(())
        }
        Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

fn render_firewall(response: &Response) -> Result<String, (u8, String)> {
    match response {
        Response::FirewallSnapshot(snapshot) => Ok(render::firewall(snapshot)),
        Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

async fn storage_command(path: &std::path::Path, command: &StorageCommand) -> CliResult {
    let mut client = connect(path).await?;
    match command {
        StorageCommand::List => {
            let response = client
                .call(&Request::Volumes)
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::Volumes(snapshot) => {
                    println!("{}", render::volumes(&snapshot));
                    Ok(())
                }
                Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        StorageCommand::Mount(args) => {
            if !args.yes {
                confirm(&format!(
                    "Mount {} on {}?{}",
                    args.device,
                    args.target,
                    if args.read_only { " Read-only." } else { "" }
                ))?;
            }
            volume_change(
                &mut client,
                Request::VolumesMount(anth_wire::VolumeMountRequest {
                    device: args.device.clone(),
                    target: args.target.clone(),
                    fs_type: args.fs_type.clone(),
                    read_only: args.read_only,
                    allow_removable: args.allow_removable,
                }),
            )
            .await
        }
        StorageCommand::Unmount(args) => {
            if !args.yes {
                confirm(&format!("Unmount {}?", args.target))?;
            }
            volume_change(
                &mut client,
                Request::VolumesUnmount(anth_wire::VolumeUnmountRequest {
                    target: args.target.clone(),
                }),
            )
            .await
        }
        StorageCommand::Partition { command } => match command {
            StoragePartitionCommand::Create(args) => {
                if !args.yes {
                    confirm(&format!(
                        "Create a {} partition of {} bytes on {}?",
                        args.kind.as_str(),
                        args.size_bytes,
                        args.disk
                    ))?;
                }
                volume_change(
                    &mut client,
                    Request::VolumePartitionCreate(anth_wire::VolumePartitionCreate {
                        disk: args.disk.clone(),
                        kind: args.kind.into(),
                        start_bytes: args.start_bytes,
                        size_bytes: args.size_bytes,
                    }),
                )
                .await
            }
            StoragePartitionCommand::Resize(args) => {
                if !args.yes {
                    confirm(&format!(
                        "Resize partition {} on {} to {} bytes?",
                        args.number, args.disk, args.size_bytes
                    ))?;
                }
                volume_change(
                    &mut client,
                    Request::VolumePartitionResize(anth_wire::VolumePartitionResize {
                        disk: args.disk.clone(),
                        number: args.number,
                        size_bytes: args.size_bytes,
                    }),
                )
                .await
            }
            StoragePartitionCommand::Delete(args) => {
                if !args.yes {
                    confirm(&format!(
                        "Delete partition {} on {}? This cannot be undone.",
                        args.number, args.disk
                    ))?;
                }
                volume_change(
                    &mut client,
                    Request::VolumePartitionDelete(anth_wire::VolumePartitionDelete {
                        disk: args.disk.clone(),
                        number: args.number,
                    }),
                )
                .await
            }
        },
        StorageCommand::Format(args) => {
            if !args.yes {
                confirm(&format!(
                    "Format {} as {}? This destroys its contents.",
                    args.partition, args.fs_type
                ))?;
            }
            volume_change(
                &mut client,
                Request::VolumeFormat(anth_wire::VolumeFormatRequest {
                    partition: args.partition.clone(),
                    fs_type: args.fs_type.clone(),
                    label: args.label.clone(),
                }),
            )
            .await
        }
    }
}

async fn volume_change(client: &mut Client, request: Request) -> CliResult {
    let response = client
        .call(&request)
        .await
        .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
    match response {
        Response::VolumeChanged { message } => {
            println!("{message}");
            Ok(())
        }
        Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

async fn account_command(path: &std::path::Path, command: &AccountCommand) -> CliResult {
    let mut client = connect(path).await?;
    match command {
        AccountCommand::List => {
            let response = client
                .call(&Request::Accounts)
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::Accounts(snapshot) => {
                    println!("{}", render::accounts(&snapshot));
                    Ok(())
                }
                Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        AccountCommand::Create(args) => {
            account_change(
                &mut client,
                Request::AccountCreate(anth_wire::AccountCreate {
                    name: args.name.clone(),
                    uid: args.uid,
                    gid: args.gid,
                    gecos: args.gecos.clone(),
                    home: args.home.clone(),
                    shell: args.shell.clone(),
                    system: args.system,
                }),
            )
            .await
        }
        AccountCommand::Delete(args) => {
            if !args.yes {
                confirm(&format!(
                    "Delete user {}?{}",
                    args.name,
                    if args.remove_home {
                        " Its home directory will be removed too."
                    } else {
                        ""
                    }
                ))?;
            }
            account_change(
                &mut client,
                Request::AccountDelete(anth_wire::AccountDelete {
                    name: args.name.clone(),
                    remove_home: args.remove_home,
                }),
            )
            .await
        }
        AccountCommand::Password(args) => {
            let password = read_password_stdin()?;
            account_change(
                &mut client,
                Request::AccountSetPassword(anth_wire::AccountSetPassword {
                    name: args.name.clone(),
                    password: anth_wire::SecretString::new(password),
                }),
            )
            .await
        }
        AccountCommand::Group { command } => match command {
            AccountGroupCommand::Create(args) => {
                account_change(
                    &mut client,
                    Request::AccountGroupCreate(anth_wire::AccountGroupCreate {
                        name: args.name.clone(),
                        gid: args.gid,
                        system: args.system,
                    }),
                )
                .await
            }
            AccountGroupCommand::Delete(args) => {
                if !args.yes {
                    confirm(&format!("Delete group {}?", args.name))?;
                }
                account_change(
                    &mut client,
                    Request::AccountGroupDelete(anth_wire::AccountGroupDelete {
                        name: args.name.clone(),
                    }),
                )
                .await
            }
        },
    }
}

async fn account_change(client: &mut Client, request: Request) -> CliResult {
    let response = client
        .call(&request)
        .await
        .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
    match response {
        Response::AccountChanged { message } => {
            println!("{message}");
            Ok(())
        }
        Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

/// Reads one password line from stdin. It is never accepted as an argument:
/// argv is visible in `/proc` and in shell history.
fn read_password_stdin() -> Result<String, (u8, String)> {
    use std::io::{IsTerminal, Read};

    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err((
            2,
            "pipe the password on stdin; it is never read from a terminal or an argument"
                .to_string(),
        ));
    }
    let mut input = String::new();
    stdin
        .lock()
        .read_to_string(&mut input)
        .map_err(|error| (1, format!("reading the password: {error}")))?;
    if input.ends_with('\n') {
        input.pop();
        if input.ends_with('\r') {
            input.pop();
        }
    }
    if input.contains('\n') || input.contains('\r') {
        return Err((2, "the password must be one line".to_string()));
    }
    if input.is_empty() {
        return Err((2, "the password must not be empty".to_string()));
    }
    Ok(input)
}

/// Repeats one request on an interval until Ctrl-C.
///
/// A terminal gets the screen cleared for a stable picture; a pipe gets a blank
/// line between snapshots so the output stays appendable.
async fn watch_loop(
    client: &mut Client,
    interval: &str,
    request: &Request,
    format: fn(&Response) -> Result<String, (u8, String)>,
) -> CliResult {
    let interval = parse_interval(interval).map_err(|error| (2, error))?;
    let terminal = std::io::stdout().is_terminal();
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut first = true;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = ticker.tick() => {
                let response = client
                    .call(request)
                    .await
                    .map_err(|error| (1, error.to_string()))?;
                let text = format(&response)?;
                if terminal {
                    println!("\x1b[2J\x1b[H{text}");
                } else {
                    if !first {
                        println!();
                    }
                    println!("{text}");
                }
                std::io::stdout().flush().ok();
                first = false;
            }
        }
    }
}

async fn process_command(path: &std::path::Path, command: &ProcessCommand) -> CliResult {
    let mut client = connect(path).await?;

    match command {
        ProcessCommand::List {
            limit,
            cursor,
            watch,
        } => {
            let request = Request::Processes {
                limit: *limit,
                cursor: cursor.clone(),
            };
            let Some(interval) = watch else {
                let response = client
                    .call(&request)
                    .await
                    .map_err(|error| (1, error.to_string()))?;
                println!("{}", render_processes(&response)?);
                return Ok(());
            };
            watch_loop(&mut client, interval, &request, render_processes).await
        }
        ProcessCommand::Signal {
            pid,
            signal,
            group,
            yes,
        } => {
            let signal = anth_wire::ProcessSignal::parse(signal)
                .ok_or_else(|| (2, format!("`{signal}` is not an allowed signal")))?;
            let target = find_process(&mut client, *pid).await?;
            if !*yes {
                confirm(&format!(
                    "Send {} to process {pid} ({})?",
                    signal.as_str(),
                    target.name
                ))?;
            }
            let response = client
                .call(&Request::ProcessSignal {
                    pid: *pid,
                    start_ticks: target.start_ticks,
                    signal,
                    group: *group,
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::ProcessSignalled { pid, signal } => {
                    println!("sent {} to {pid}", signal.as_str());
                    Ok(())
                }
                Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
        ProcessCommand::Spawn { program, args, cwd } => {
            let response = client
                .call(&Request::ProcessSpawn {
                    program: program.clone(),
                    args: args.clone(),
                    cwd: cwd.clone(),
                })
                .await
                .map_err(|error| (error.kind.exit_code(), error.to_string()))?;
            match response {
                Response::ProcessStarted(started) => {
                    println!("started {} (pid {})", started.program, started.pid);
                    Ok(())
                }
                Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
                other => Err((1, format!("unexpected response: {other:?}"))),
            }
        }
    }
}

/// Finds one process so a signal can carry its `start_ticks`.
async fn find_process(
    client: &mut Client,
    pid: u32,
) -> Result<anth_wire::ProcessInfo, (u8, String)> {
    let mut cursor = None;
    for _ in 0..20 {
        let response = client
            .call(&Request::Processes {
                limit: Some(1000),
                cursor: cursor.clone(),
            })
            .await
            .map_err(|error| (1, error.to_string()))?;
        match response {
            Response::Processes(snapshot) => {
                if let Some(found) = snapshot.page.items.iter().find(|row| row.pid == pid) {
                    return Ok(found.clone());
                }
                match snapshot.page.next {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
            Response::Error(error) => return Err((error.kind.exit_code(), error.to_string())),
            other => return Err((1, format!("unexpected response: {other:?}"))),
        }
    }
    Err((1, format!("no process `{pid}`")))
}

fn render_processes(response: &Response) -> Result<String, (u8, String)> {
    match response {
        Response::Processes(snapshot) => Ok(render::process_list(&snapshot.page)),
        Response::Error(error) => Err((error.kind.exit_code(), error.to_string())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

/// Asks on a TTY; refuses instead of assuming an answer anywhere else.
fn confirm(prompt: &str) -> CliResult {
    if !std::io::stdin().is_terminal() {
        return Err((
            2,
            "refusing to continue without --yes when stdin is not a terminal".to_string(),
        ));
    }
    print!("{prompt} [y/N] ");
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|error| (1, error.to_string()))?;
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err((1, "aborted".to_string()))
    }
}

fn render_response(response: &Response) -> Result<String, (u8, String)> {
    match response {
        Response::HostInfo(info) => Ok(render::host_info(info)),
        Response::Error(error) => Err((error.kind.exit_code(), error.message.clone())),
        other => Err((1, format!("unexpected response: {other:?}"))),
    }
}

fn client_settings(config: Option<&Config>, cli: &Cli) -> (Settings, Vec<String>) {
    let default = config
        .map(|config| Settings {
            level: config.log.level,
            format: config.log.format,
            out: config.log.out,
        })
        .unwrap_or(Settings {
            level: Level::Info,
            format: Format::Human,
            out: Out::Stdout,
        });
    Settings::resolve_with(
        default,
        cli.log_format.as_deref(),
        cli.log_level.as_deref(),
        cli.log_out.as_deref(),
        &|name| std::env::var(name).ok(),
    )
}

fn prepare_state_dir(path: &std::path::Path) -> CliResult {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(path)
        .map_err(|error| (1, format!("creating {}: {error}", path.display())))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|error| (1, format!("setting mode on {}: {error}", path.display())))?;
    Ok(())
}
