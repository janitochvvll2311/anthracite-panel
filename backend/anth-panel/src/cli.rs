use anth_proto::{ArchiveFormat, FirewallProtocol, ProcessSignal};
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(name = "anth-panel")]
#[command(version = "0.1.0", about = "Anthracite Panel", long_about = None)]
pub struct Cli {
    #[arg(
        name = "config",
        short,
        long,
        global = true,
        value_name = "FILE",
        help = "Path to the configuration file"
    )]
    pub config_path: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn parse() -> Self {
        Parser::parse()
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Serve the Unix-socket daemon.
    Daemon,
    /// Serve the HTTP API and the embedded web UI.
    Api(EndpointArgs),
    /// Serve the daemon and the HTTP API from a single process.
    Run(EndpointArgs),
    /// Send a request to the running daemon and print the response.
    Echo {
        #[arg(help = "Message to echo")]
        message: String,
    },
    /// Print the operating system and resources of the daemon host as a table.
    HostInfo(WatchArgs),
    /// Inspect processes on the daemon host and optionally signal one.
    Process(ProcessArgs),
    /// Inspect network interfaces and optionally bring one up or down.
    Network(NetworkArgs),
    /// Inspect and configure the managed nftables firewall.
    Firewall(FirewallArgs),
    /// Browse, mount and unmount storage volumes.
    Volume(VolumeArgs),
    /// Inspect and manage users and groups on the daemon host.
    Account(AccountArgs),
    /// Browse and manage files on the daemon host.
    File(FileArgs),
    /// Open an interactive terminal on the daemon host.
    Terminal(TerminalArgs),
    /// Manage the panel login: enable it or change the password.
    Auth(AuthArgs),
    /// Read and change the panel configuration.
    Config(ConfigArgs),
}

#[derive(Args, Debug)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

/// `auth` subcommands. They edit the config file directly; a running server
/// picks the change up on its next start, and `auth set-password` says so.
#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Turn the login on or off.
    Enable(AuthToggleArgs),
    /// Change the panel password.
    SetPassword(AuthSetPasswordArgs),
    /// Print whether the login is on and which account it uses.
    Status,
}

#[derive(Args, Debug)]
pub struct AuthToggleArgs {
    /// Turn the login off instead of on.
    #[arg(long)]
    pub disable: bool,
}

#[derive(Args, Debug)]
pub struct AuthSetPasswordArgs {
    /// Read the password from stdin instead of prompting on the terminal.
    #[arg(long)]
    pub stdin: bool,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// `config` subcommands. `get` prints the whole file; `set` changes one key.
#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Print the configuration as TOML.
    Get,
    /// Change one key and save the file.
    Set(ConfigSetArgs),
}

#[derive(Args, Debug)]
pub struct ConfigSetArgs {
    #[arg(
        value_name = "KEY",
        help = "Dotted key, e.g. files.write or accounts.control"
    )]
    pub key: String,

    #[arg(value_name = "VALUE", help = "New value, parsed as a TOML literal")]
    pub value: String,
}

#[derive(Args, Debug)]
pub struct ProcessArgs {
    #[command(subcommand)]
    pub command: ProcessCommand,
}

/// `process` subcommands. `list` keeps the short `ls` alias.
#[derive(Subcommand, Debug)]
pub enum ProcessCommand {
    /// List processes as a table.
    #[command(alias = "ls")]
    List(WatchArgs),
    /// Send an allowlisted signal to one process.
    Signal(ProcessSignalArgs),
    /// Start a program from the configured allowlist.
    Spawn(ProcessSpawnArgs),
}

#[derive(Args, Debug)]
pub struct ProcessSignalArgs {
    #[arg(value_name = "PID", help = "Process id to signal")]
    pub pid: u32,

    #[arg(
        value_name = "SIGNAL",
        value_parser = parse_signal,
        help = "Signal name: term, kill, hup, int, quit, stop, cont"
    )]
    pub signal: ProcessSignal,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

fn parse_signal(value: &str) -> Result<ProcessSignal, String> {
    value.parse()
}

#[derive(Args, Debug)]
#[command(
    after_help = "Everything after PROGRAM is passed to it verbatim, so put \
                          `--cwd` and `--yes` before the program."
)]
pub struct ProcessSpawnArgs {
    #[arg(
        value_name = "PROGRAM",
        help = "Absolute program path; must appear in `[process] spawn_allow`"
    )]
    pub program: String,

    #[arg(
        value_name = "ARG",
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "Arguments passed to the program"
    )]
    pub args: Vec<String>,

    #[arg(
        long,
        value_name = "DIR",
        help = "Absolute working directory for the new process"
    )]
    pub cwd: Option<String>,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct NetworkArgs {
    #[command(subcommand)]
    pub command: NetworkCommand,
}

/// `network` subcommands. `list` keeps the short `ls` alias.
#[derive(Subcommand, Debug)]
pub enum NetworkCommand {
    /// List network interfaces as a table.
    #[command(alias = "ls")]
    List(WatchArgs),
    /// Bring one interface up.
    Up(NetworkStateArgs),
    /// Bring one interface down.
    Down(NetworkStateArgs),
}

#[derive(Args, Debug)]
pub struct NetworkStateArgs {
    #[arg(value_name = "INTERFACE", help = "Network interface name, e.g. eth0")]
    pub name: String,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct FirewallArgs {
    #[command(subcommand)]
    pub command: FirewallCommand,
}

/// `firewall` subcommands. `status` keeps the short `list` alias.
#[derive(Subcommand, Debug)]
pub enum FirewallCommand {
    /// Show the managed firewall table and its rules.
    #[command(alias = "list")]
    Status,
    /// Allow traffic to a port.
    Allow(FirewallAddArgs),
    /// Deny traffic to a port.
    Deny(FirewallAddArgs),
    /// Delete one rule by its nftables handle.
    Delete(FirewallDeleteArgs),
    /// Remove every rule from the managed chain.
    Flush(FirewallFlushArgs),
}

#[derive(Clone, Copy, Debug)]
pub struct PortSpec {
    pub port: u16,
    pub protocol: FirewallProtocol,
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
pub struct FirewallAddArgs {
    #[arg(
        value_name = "PORT[/PROTOCOL]",
        help = "Port to allow or deny, optionally suffixed with /tcp or /udp"
    )]
    pub spec: PortSpec,

    #[arg(
        long,
        value_name = "TEXT",
        help = "Optional rule comment (letters, digits, dot, underscore and dash)"
    )]
    pub comment: Option<String>,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct FirewallDeleteArgs {
    #[arg(value_name = "HANDLE", help = "Rule handle shown by `firewall status`")]
    pub handle: u64,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct FirewallFlushArgs {
    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct VolumeArgs {
    #[command(subcommand)]
    pub command: VolumeCommand,
}

/// `volume` subcommands. `unmount` keeps `umount` as an alias because that is
/// the name of the binary most operators already type.
#[derive(Subcommand, Debug)]
pub enum VolumeCommand {
    /// List block devices and mounts as tables.
    #[command(alias = "ls")]
    List,
    /// Mount a device on a directory.
    Mount(VolumeMountArgs),
    /// Unmount a mounted directory.
    #[command(alias = "umount")]
    Unmount(VolumeUnmountArgs),
}

#[derive(Args, Debug)]
pub struct VolumeMountArgs {
    #[arg(
        value_name = "DEVICE",
        help = "Device path or kernel name, e.g. /dev/sdb1"
    )]
    pub device: String,

    #[arg(
        value_name = "TARGET",
        help = "Existing directory below volumes.mount_root"
    )]
    pub target: String,

    #[arg(
        long = "type",
        value_name = "FSTYPE",
        help = "Filesystem type; detected from the device when omitted"
    )]
    pub fs_type: Option<String>,

    #[arg(long, help = "Mount read-only")]
    pub read_only: bool,

    #[arg(
        long,
        help = "Allow mounting removable media, which is refused by default"
    )]
    pub allow_removable: bool,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct VolumeUnmountArgs {
    #[arg(value_name = "TARGET", help = "Mountpoint to unmount")]
    pub target: String,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct AccountArgs {
    #[command(subcommand)]
    pub command: AccountCommand,
}

/// `account` subcommands, split into `user` and `group` so the flag sets stay
/// disjoint. There are deliberately no aliases: `user` alone would read like an
/// unrelated command in `--help`.
#[derive(Subcommand, Debug)]
pub enum AccountCommand {
    /// List users and groups as tables.
    #[command(alias = "ls")]
    List,
    /// Create, change or delete a user.
    User(AccountUserArgs),
    /// Create, change or delete a group.
    Group(AccountGroupArgs),
}

#[derive(Args, Debug)]
pub struct AccountUserArgs {
    #[command(subcommand)]
    pub command: AccountUserCommand,
}

#[derive(Subcommand, Debug)]
pub enum AccountUserCommand {
    /// Create a locked local user.
    Create(AccountUserCreateArgs),
    /// Change a local user.
    Modify(AccountUserModifyArgs),
    /// Delete a local user.
    Delete(AccountUserDeleteArgs),
    /// Set a local user's password, read from the terminal or stdin.
    Password(AccountUserPasswordArgs),
}

#[derive(Args, Debug)]
pub struct AccountUserCreateArgs {
    #[arg(value_name = "NAME", help = "Login name")]
    pub name: String,

    #[arg(long, value_name = "UID", help = "Explicit user id")]
    pub uid: Option<u32>,

    #[arg(long, value_name = "GID", help = "Existing primary group id")]
    pub gid: Option<u32>,

    #[arg(long = "comment", value_name = "TEXT", help = "Description (GECOS)")]
    pub gecos: Option<String>,

    #[arg(
        long,
        value_name = "DIR",
        help = "Home directory, below accounts.home_root"
    )]
    pub home: Option<String>,

    #[arg(long, value_name = "SHELL", help = "Login shell from the allowlist")]
    pub shell: Option<String>,

    #[arg(long, help = "Create a system account")]
    pub system: bool,
}

#[derive(Args, Debug)]
pub struct AccountUserModifyArgs {
    #[arg(value_name = "NAME", help = "Login name")]
    pub name: String,

    #[arg(long = "comment", value_name = "TEXT", help = "Description (GECOS)")]
    pub gecos: Option<String>,

    #[arg(long, value_name = "SHELL", help = "Login shell from the allowlist")]
    pub shell: Option<String>,

    #[arg(
        long,
        value_name = "DIR",
        help = "Home directory, below accounts.home_root"
    )]
    pub home: Option<String>,

    #[arg(long, value_name = "GID", help = "Primary group id")]
    pub gid: Option<u32>,

    #[arg(long, help = "Lock the account", conflicts_with = "unlock")]
    pub lock: bool,

    #[arg(long, help = "Unlock the account")]
    pub unlock: bool,
}

#[derive(Args, Debug)]
pub struct AccountUserDeleteArgs {
    #[arg(value_name = "NAME", help = "Login name")]
    pub name: String,

    #[arg(
        long,
        help = "Also remove the home directory, which must be inside accounts.home_root"
    )]
    pub remove_home: bool,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct AccountUserPasswordArgs {
    #[arg(value_name = "NAME", help = "Login name")]
    pub name: String,

    #[arg(
        long,
        help = "Read the password from stdin, which must be piped; never from argv"
    )]
    pub stdin: bool,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct AccountGroupArgs {
    #[command(subcommand)]
    pub command: AccountGroupCommand,
}

#[derive(Subcommand, Debug)]
pub enum AccountGroupCommand {
    /// Create a local group.
    Create(AccountGroupCreateArgs),
    /// Change a group's gid or replace its member list.
    Modify(AccountGroupModifyArgs),
    /// Delete a local group.
    Delete(AccountGroupDeleteArgs),
}

#[derive(Args, Debug)]
pub struct AccountGroupCreateArgs {
    #[arg(value_name = "NAME", help = "Group name")]
    pub name: String,

    #[arg(long, value_name = "GID", help = "Explicit group id")]
    pub gid: Option<u32>,

    #[arg(long, help = "Create a system group")]
    pub system: bool,
}

#[derive(Args, Debug)]
pub struct AccountGroupModifyArgs {
    #[arg(value_name = "NAME", help = "Group name")]
    pub name: String,

    #[arg(long, value_name = "GID", help = "New group id")]
    pub gid: Option<u32>,

    #[arg(
        long,
        value_name = "USERS",
        value_delimiter = ',',
        help = "Replace the member list with these users"
    )]
    pub members: Option<Vec<String>>,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct AccountGroupDeleteArgs {
    #[arg(value_name = "NAME", help = "Group name")]
    pub name: String,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct FileArgs {
    #[command(subcommand)]
    pub command: FileCommand,
}

/// `file` subcommands. There are deliberately no aliases.
#[derive(Subcommand, Debug)]
pub enum FileCommand {
    /// List a directory as a table.
    List(FileListArgs),
    /// Read a file and write its bytes to stdout or to --output.
    Read(FileReadArgs),
    /// Create a directory.
    Mkdir(FileMkdirArgs),
    /// Upload a local file to the daemon host.
    Upload(FileUploadArgs),
    /// Rename a file or directory.
    Rename(FileRenameArgs),
    /// Delete a file or directory permanently.
    Delete(FileDeleteArgs),
    /// Move a file or directory to the freedesktop trash.
    Trash(FileTrashArgs),
    /// Inspect or empty the trash.
    TrashBin(FileTrashBinArgs),
    /// Compress one entry into an archive.
    Archive(FileArchiveArgs),
    /// Inspect or extract an archive.
    Unarchive(FileUnarchiveArgs),
    /// Replace a text file with the contents of a local file.
    Write(FileWriteArgs),
}

#[derive(Args, Debug)]
pub struct FileWriteArgs {
    #[arg(value_name = "PATH", help = "File to overwrite")]
    pub path: String,

    #[arg(
        value_name = "SOURCE",
        help = "Local file to read; `-` reads standard input"
    )]
    pub source: String,

    #[arg(
        long,
        value_name = "MILLIS",
        help = "Only save when the file still has this modification time"
    )]
    pub expected_modified_ms: Option<u64>,
}

#[derive(Args, Debug)]
pub struct FileArchiveArgs {
    #[arg(value_name = "PATH", help = "Entry to compress")]
    pub path: String,

    #[arg(
        short,
        long,
        value_name = "PATH",
        help = "Archive to write; defaults to PATH plus a suffix"
    )]
    pub output: Option<String>,

    #[arg(
        short,
        long,
        value_name = "FORMAT",
        value_enum,
        help = "Archive format; inferred from --output when omitted"
    )]
    pub format: Option<ArchiveFormatArg>,
}

#[derive(Args, Debug)]
pub struct FileUnarchiveArgs {
    /// List the members without extracting anything.
    #[arg(long, conflicts_with_all = ["into"])]
    pub list: bool,

    #[arg(value_name = "PATH", help = "Archive to read")]
    pub path: String,

    #[arg(
        short,
        long,
        value_name = "PATH",
        help = "Directory to extract into; defaults to PATH without its suffix"
    )]
    pub into: Option<String>,
}

/// CLI spelling of an archive format. Kept separate from the protocol enum so
/// clap owns the flag names.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormatArg {
    Zip,
    Tar,
    #[value(name = "tar.gz")]
    TarGz,
    #[value(name = "tar.xz")]
    TarXz,
}

impl ArchiveFormatArg {
    pub fn to_protocol(self) -> ArchiveFormat {
        match self {
            Self::Zip => ArchiveFormat::Zip,
            Self::Tar => ArchiveFormat::Tar,
            Self::TarGz => ArchiveFormat::TarGz,
            Self::TarXz => ArchiveFormat::TarXz,
        }
    }
}

#[derive(Args, Debug)]
pub struct FileListArgs {
    #[arg(
        value_name = "PATH",
        default_value = "/",
        help = "Virtual path below the configured filesystem root"
    )]
    pub path: String,
}

#[derive(Args, Debug)]
pub struct FileReadArgs {
    #[arg(value_name = "PATH", help = "File to read")]
    pub path: String,

    #[arg(
        short,
        long,
        value_name = "FILE",
        help = "Write the bytes to FILE instead of stdout"
    )]
    pub output: Option<String>,
}

#[derive(Args, Debug)]
pub struct FileMkdirArgs {
    #[arg(value_name = "PATH", help = "Directory to create")]
    pub path: String,

    #[arg(long, help = "Create missing parent directories")]
    pub parents: bool,
}

#[derive(Args, Debug)]
pub struct FileUploadArgs {
    #[arg(value_name = "LOCAL", help = "Local file to upload")]
    pub local: String,

    #[arg(value_name = "DEST", help = "Destination virtual path")]
    pub dest: String,

    #[arg(long, help = "Replace DEST when it already exists")]
    pub overwrite: bool,
}

#[derive(Args, Debug)]
pub struct FileRenameArgs {
    #[arg(value_name = "FROM", help = "Existing virtual path")]
    pub from: String,

    #[arg(value_name = "TO", help = "New virtual path")]
    pub to: String,
}

#[derive(Args, Debug)]
pub struct FileDeleteArgs {
    #[arg(value_name = "PATH", help = "Entry to delete")]
    pub path: String,

    #[arg(long, help = "Delete a directory and everything below it")]
    pub recursive: bool,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct FileTrashArgs {
    #[arg(value_name = "PATH", help = "Entry to move to the trash")]
    pub path: String,
}

#[derive(Args, Debug)]
pub struct FileTrashBinArgs {
    #[command(subcommand)]
    pub command: TrashBinCommand,
}

#[derive(Subcommand, Debug)]
pub enum TrashBinCommand {
    /// List what the trash holds.
    List,
    /// Put one entry back where it came from.
    Restore(TrashRestoreArgs),
    /// Delete everything in the trash.
    Empty(TrashEmptyArgs),
}

#[derive(Args, Debug)]
pub struct TrashRestoreArgs {
    #[arg(
        value_name = "ID",
        help = "Trash identifier, as printed by `trash-bin list`"
    )]
    pub id: String,

    #[arg(long, help = "Replace the entry that now occupies the original path")]
    pub overwrite: bool,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug)]
pub struct TrashEmptyArgs {
    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

/// `--watch` takes an optional interval: bare, it repeats every 2s.
#[derive(Args, Debug, Default)]
pub struct WatchArgs {
    #[arg(
        long,
        value_name = "INTERVAL",
        num_args = 0..=1,
        default_missing_value = "2s",
        help = "Keep reading every INTERVAL (500ms, 2s, 1m) instead of printing once"
    )]
    pub watch: Option<String>,
}

/// Initial size for `terminal`: the session starts at the local terminal's
/// size, or at the explicit values when either flag is given.
#[derive(Args, Debug, Default)]
pub struct TerminalArgs {
    #[arg(
        long,
        value_name = "COLS",
        help = "Initial terminal width in columns; defaults to the local terminal width"
    )]
    pub cols: Option<u16>,

    #[arg(
        long,
        value_name = "ROWS",
        help = "Initial terminal height in rows; defaults to the local terminal height"
    )]
    pub rows: Option<u16>,
}

#[derive(Args, Debug, Default)]
pub struct EndpointArgs {
    #[arg(
        long,
        value_name = "ADDR",
        help = "Address the HTTP API binds to; defaults to ANTH_PANEL_ADDR or 0.0.0.0:8080"
    )]
    pub addr: Option<String>,
}
