//! The Anthracite Panel wire contract: data transfer objects only.
//!
//! Every type here is the JSON body of one HTTP resource. The crate has no
//! transport, so a custom client and a third-party implementation share the
//! same shapes.

mod accounts;
mod actions;
mod error;
mod firewall;
mod interval;
mod message;
mod network;
mod page;
mod process;
mod terminal;
mod volumes;

pub use accounts::{
    AccountCreate, AccountDelete, AccountGroup, AccountGroupCreate, AccountGroupDelete,
    AccountSetPassword, AccountSnapshot, AccountSource, AccountUser, SecretString,
};
pub use actions::{
    ArchiveCreated, ArchiveExtracted, Change, FileDeleted, FileRestored, FileTrashed,
    ProcessSignalled, Rebooting, Restarting, TrashEmptied,
};
pub use error::{ErrorKind, WireError};
pub use firewall::{
    FirewallAction, FirewallChain, FirewallChainState, FirewallProtocol, FirewallRule,
    FirewallSnapshot,
};
pub use interval::{DEFAULT_INTERVAL, MAX_INTERVAL, MIN_INTERVAL, parse_interval};
pub use message::{
    ArchiveEntry, ArchiveFormat, ArchiveListing, DEFAULT_DOWNLOAD_CHUNK, FileChunk, FileEntry,
    FileKind, FileWritten, Hello, HostInfo, LoadAverage, MAX_DOWNLOAD_CHUNK, MemoryInfo, OsInfo,
    PROTOCOL_VERSION, TrashEntry, TrashListing,
};
pub use network::{
    NetworkAddress, NetworkCreate, NetworkEdit, NetworkInterface, NetworkRoute, NetworkSnapshot,
};
pub use page::{DEFAULT_PAGE_LIMIT, MAX_CURSOR_LEN, MAX_PAGE_LIMIT, Page, PageRequest};
pub use process::{
    MAX_SPAWN_ARG_BYTES, MAX_SPAWN_ARGS, ProcessInfo, ProcessSignal, ProcessSnapshot,
    ProcessStarted,
};
pub use terminal::{
    DEFAULT_TERMINAL_SIZE, MAX_TERMINAL_CHUNK, MAX_TERMINAL_DIMENSION, MAX_TERMINAL_INPUT,
    MAX_TERMINAL_WAIT_MS, MIN_TERMINAL_DIMENSION, TerminalClientMessage, TerminalServerMessage,
    TerminalSize,
};
pub use volumes::{
    PartitionKind, VolumeDevice, VolumeFormatRequest, VolumeKind, VolumeMount, VolumeMountRequest,
    VolumePartitionCreate, VolumePartitionDelete, VolumePartitionResize, VolumeSnapshot,
    VolumeUnmountRequest, VolumeUsage,
};

#[cfg(test)]
mod tests;
