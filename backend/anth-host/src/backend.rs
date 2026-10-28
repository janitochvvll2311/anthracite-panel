//! Backend ports for the host boundary.
//!
//! `anth-panel` talks to these traits. The system implementations in
//! `network`/`firewall` (and the modules added in later phases) perform real
//! host operations; `demo` implements the same contracts in memory.

use anth_wire::{
    FirewallAction, FirewallChain, FirewallProtocol, FirewallSnapshot, NetworkCreate, NetworkEdit,
    NetworkSnapshot, WireError,
};

use crate::firewall::SystemFirewall;
use crate::network::SystemNetwork;

pub trait NetworkService: Send + Sync {
    fn snapshot(&self) -> Result<NetworkSnapshot, WireError>;
    fn set_state(&self, name: &str, up: bool) -> Result<String, WireError>;
    fn create(&self, args: &NetworkCreate) -> Result<String, WireError>;
    fn edit(&self, args: &NetworkEdit) -> Result<String, WireError>;
    fn rename(&self, name: &str, new_name: &str) -> Result<String, WireError>;
    fn delete(&self, name: &str, peer: Option<&str>) -> Result<String, WireError>;
}

pub trait FirewallService: Send + Sync {
    fn snapshot(&self) -> Result<FirewallSnapshot, WireError>;
    fn add(
        &self,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError>;
    fn update(
        &self,
        handle: u64,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError>;
    fn delete(&self, handle: u64) -> Result<String, WireError>;
    fn flush(&self, chain: FirewallChain) -> Result<String, WireError>;
    fn set_policy(&self, chain: FirewallChain, policy: FirewallAction)
    -> Result<String, WireError>;
}

impl NetworkService for SystemNetwork {
    fn snapshot(&self) -> Result<NetworkSnapshot, WireError> {
        SystemNetwork::snapshot(self)
    }
    fn set_state(&self, name: &str, up: bool) -> Result<String, WireError> {
        SystemNetwork::set_state(self, name, up)
    }
    fn create(&self, args: &NetworkCreate) -> Result<String, WireError> {
        SystemNetwork::create(self, args)
    }
    fn edit(&self, args: &NetworkEdit) -> Result<String, WireError> {
        SystemNetwork::edit(self, args)
    }
    fn rename(&self, name: &str, new_name: &str) -> Result<String, WireError> {
        SystemNetwork::rename(self, name, new_name)
    }
    fn delete(&self, name: &str, peer: Option<&str>) -> Result<String, WireError> {
        SystemNetwork::delete(self, name, peer)
    }
}

impl FirewallService for SystemFirewall {
    fn snapshot(&self) -> Result<FirewallSnapshot, WireError> {
        SystemFirewall::snapshot(self)
    }
    fn add(
        &self,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        SystemFirewall::add(self, chain, action, protocol, port, comment)
    }
    fn update(
        &self,
        handle: u64,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        SystemFirewall::update(self, handle, chain, action, protocol, port, comment)
    }
    fn delete(&self, handle: u64) -> Result<String, WireError> {
        SystemFirewall::delete(self, handle)
    }
    fn flush(&self, chain: FirewallChain) -> Result<String, WireError> {
        SystemFirewall::flush(self, chain)
    }
    fn set_policy(
        &self,
        chain: FirewallChain,
        policy: FirewallAction,
    ) -> Result<String, WireError> {
        SystemFirewall::set_policy(self, chain, policy)
    }
}

pub trait FilesService: Send + Sync {
    fn list(
        &self,
        path: &str,
        request: &anth_wire::PageRequest,
    ) -> Result<anth_wire::Page<anth_wire::FileEntry>, WireError>;
    fn read(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<anth_wire::FileChunk, WireError>;
    fn delete(&self, path: &str) -> Result<String, WireError>;
    fn write(
        &self,
        path: &str,
        content: &str,
        create: bool,
    ) -> Result<anth_wire::FileWritten, WireError>;
    fn write_bytes(
        &self,
        path: &str,
        content: &[u8],
        create: bool,
    ) -> Result<anth_wire::FileWritten, WireError>;
    fn trash(&self, path: &str) -> Result<(String, String), WireError>;
    fn trash_listing(&self) -> Result<anth_wire::TrashListing, WireError>;
    fn restore(&self, id: &str, overwrite: bool) -> Result<(String, String), WireError>;
    fn empty_trash(&self) -> Result<usize, WireError>;
    fn create_archive(
        &self,
        path: &str,
        destination: Option<&str>,
        format: Option<anth_wire::ArchiveFormat>,
    ) -> Result<String, WireError>;
    fn list_archive(&self, path: &str) -> Result<anth_wire::ArchiveListing, WireError>;
    fn extract_archive(
        &self,
        path: &str,
        into: Option<&str>,
    ) -> Result<(String, Vec<String>), WireError>;
    fn image(&self, path: &str) -> Result<(Vec<u8>, &'static str), WireError>;
    fn download(&self, path: &str) -> Result<Vec<u8>, WireError>;
    fn download_chunk(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<crate::files::DownloadChunk, WireError>;
}

pub trait ProcessesService: Send + Sync {
    fn snapshot(
        &self,
        request: &anth_wire::PageRequest,
    ) -> Result<anth_wire::ProcessSnapshot, WireError>;
    fn signal(
        &self,
        pid: u32,
        start_ticks: u64,
        signal: anth_wire::ProcessSignal,
        group: bool,
    ) -> Result<(), WireError>;
    fn spawn(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> Result<anth_wire::ProcessStarted, WireError>;
}

impl FilesService for crate::files::SystemFiles {
    fn list(
        &self,
        path: &str,
        request: &anth_wire::PageRequest,
    ) -> Result<anth_wire::Page<anth_wire::FileEntry>, WireError> {
        crate::files::SystemFiles::list(self, path, request)
    }
    fn read(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<anth_wire::FileChunk, WireError> {
        crate::files::SystemFiles::read(self, path, offset, max_bytes)
    }
    fn delete(&self, path: &str) -> Result<String, WireError> {
        crate::files::SystemFiles::delete(self, path)
    }
    fn write(
        &self,
        path: &str,
        content: &str,
        create: bool,
    ) -> Result<anth_wire::FileWritten, WireError> {
        crate::files::SystemFiles::write(self, path, content, create)
    }
    fn write_bytes(
        &self,
        path: &str,
        content: &[u8],
        create: bool,
    ) -> Result<anth_wire::FileWritten, WireError> {
        crate::files::SystemFiles::write_bytes(self, path, content, create)
    }
    fn trash(&self, path: &str) -> Result<(String, String), WireError> {
        crate::files::SystemFiles::trash(self, path)
    }
    fn trash_listing(&self) -> Result<anth_wire::TrashListing, WireError> {
        crate::files::SystemFiles::trash_listing(self)
    }
    fn restore(&self, id: &str, overwrite: bool) -> Result<(String, String), WireError> {
        crate::files::SystemFiles::restore(self, id, overwrite)
    }
    fn empty_trash(&self) -> Result<usize, WireError> {
        crate::files::SystemFiles::empty_trash(self)
    }
    fn create_archive(
        &self,
        path: &str,
        destination: Option<&str>,
        format: Option<anth_wire::ArchiveFormat>,
    ) -> Result<String, WireError> {
        crate::files::SystemFiles::create_archive(self, path, destination, format)
    }
    fn list_archive(&self, path: &str) -> Result<anth_wire::ArchiveListing, WireError> {
        crate::files::SystemFiles::list_archive(self, path)
    }
    fn extract_archive(
        &self,
        path: &str,
        into: Option<&str>,
    ) -> Result<(String, Vec<String>), WireError> {
        crate::files::SystemFiles::extract_archive(self, path, into)
    }
    fn image(&self, path: &str) -> Result<(Vec<u8>, &'static str), WireError> {
        crate::files::SystemFiles::image(self, path)
    }
    fn download(&self, path: &str) -> Result<Vec<u8>, WireError> {
        crate::files::SystemFiles::download(self, path)
    }
    fn download_chunk(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<crate::files::DownloadChunk, WireError> {
        crate::files::SystemFiles::download_chunk(self, path, offset, max_bytes)
    }
}

impl ProcessesService for crate::process::SystemProcesses {
    fn snapshot(
        &self,
        request: &anth_wire::PageRequest,
    ) -> Result<anth_wire::ProcessSnapshot, WireError> {
        crate::process::SystemProcesses::snapshot(self, request)
    }
    fn signal(
        &self,
        pid: u32,
        start_ticks: u64,
        signal: anth_wire::ProcessSignal,
        group: bool,
    ) -> Result<(), WireError> {
        crate::process::SystemProcesses::signal(self, pid, start_ticks, signal, group)
    }
    fn spawn(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> Result<anth_wire::ProcessStarted, WireError> {
        crate::process::SystemProcesses::spawn(self, program, args, cwd)
    }
}

pub trait TerminalService: Send + Sync {
    fn enabled(&self) -> bool;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn open(
        &self,
        owner: &str,
        program: &str,
        size: anth_wire::TerminalSize,
    ) -> Result<String, crate::terminal::OpenError>;
    fn read<'a>(
        &'a self,
        owner: &'a str,
        id: &'a str,
        wait_ms: u32,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<crate::terminal::Chunk, WireError>> + Send + 'a,
        >,
    >;
    fn write(&self, owner: &str, id: &str, bytes: &[u8]) -> Result<(), WireError>;
    fn resize(&self, owner: &str, id: &str, size: anth_wire::TerminalSize)
    -> Result<(), WireError>;
    fn close(&self, owner: &str, id: &str) -> Result<(), WireError>;
    fn close_owner(&self, owner: &str) -> usize;
    fn reap(&self) -> usize;
}

impl TerminalService for crate::terminal::SystemTerminal {
    fn enabled(&self) -> bool {
        self.enabled()
    }
    fn len(&self) -> usize {
        self.len()
    }
    fn open(
        &self,
        owner: &str,
        program: &str,
        size: anth_wire::TerminalSize,
    ) -> Result<String, crate::terminal::OpenError> {
        self.open(owner, program, size)
    }
    fn read<'a>(
        &'a self,
        owner: &'a str,
        id: &'a str,
        wait_ms: u32,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<crate::terminal::Chunk, WireError>> + Send + 'a,
        >,
    > {
        Box::pin(self.read(owner, id, wait_ms))
    }
    fn write(&self, owner: &str, id: &str, bytes: &[u8]) -> Result<(), WireError> {
        self.write(owner, id, bytes)
    }
    fn resize(
        &self,
        owner: &str,
        id: &str,
        size: anth_wire::TerminalSize,
    ) -> Result<(), WireError> {
        self.resize(owner, id, size)
    }
    fn close(&self, owner: &str, id: &str) -> Result<(), WireError> {
        self.close(owner, id)
    }
    fn close_owner(&self, owner: &str) -> usize {
        self.close_owner(owner)
    }
    fn reap(&self) -> usize {
        self.reap()
    }
}

pub trait VolumesService: Send + Sync {
    fn snapshot(&self) -> anth_wire::VolumeSnapshot;
    fn mount(&self, request: &anth_wire::VolumeMountRequest) -> Result<String, WireError>;
    fn unmount(&self, request: &anth_wire::VolumeUnmountRequest) -> Result<String, WireError>;
    fn create_partition(
        &self,
        request: &anth_wire::VolumePartitionCreate,
    ) -> Result<String, WireError>;
    fn resize_partition(
        &self,
        request: &anth_wire::VolumePartitionResize,
    ) -> Result<String, WireError>;
    fn delete_partition(
        &self,
        request: &anth_wire::VolumePartitionDelete,
    ) -> Result<String, WireError>;
    fn format_partition(
        &self,
        request: &anth_wire::VolumeFormatRequest,
    ) -> Result<String, WireError>;
}

impl VolumesService for crate::volumes::SystemVolumes {
    fn snapshot(&self) -> anth_wire::VolumeSnapshot {
        crate::volumes::SystemVolumes::snapshot(self)
    }
    fn mount(&self, request: &anth_wire::VolumeMountRequest) -> Result<String, WireError> {
        crate::volumes::SystemVolumes::mount(self, request)
    }
    fn unmount(&self, request: &anth_wire::VolumeUnmountRequest) -> Result<String, WireError> {
        crate::volumes::SystemVolumes::unmount(self, request)
    }
    fn create_partition(
        &self,
        request: &anth_wire::VolumePartitionCreate,
    ) -> Result<String, WireError> {
        crate::volumes::SystemVolumes::create_partition(self, request)
    }
    fn resize_partition(
        &self,
        request: &anth_wire::VolumePartitionResize,
    ) -> Result<String, WireError> {
        crate::volumes::SystemVolumes::resize_partition(self, request)
    }
    fn delete_partition(
        &self,
        request: &anth_wire::VolumePartitionDelete,
    ) -> Result<String, WireError> {
        crate::volumes::SystemVolumes::delete_partition(self, request)
    }
    fn format_partition(
        &self,
        request: &anth_wire::VolumeFormatRequest,
    ) -> Result<String, WireError> {
        crate::volumes::SystemVolumes::format_partition(self, request)
    }
}

pub trait AccountsService: Send + Sync {
    fn snapshot(&self) -> anth_wire::AccountSnapshot;
    fn create_user(&self, request: &anth_wire::AccountCreate) -> Result<String, WireError>;
    fn delete_user(&self, request: &anth_wire::AccountDelete) -> Result<String, WireError>;
    fn set_password(&self, request: &anth_wire::AccountSetPassword) -> Result<String, WireError>;
    fn create_group(&self, request: &anth_wire::AccountGroupCreate) -> Result<String, WireError>;
    fn delete_group(&self, request: &anth_wire::AccountGroupDelete) -> Result<String, WireError>;
}

impl AccountsService for crate::accounts::SystemAccounts {
    fn snapshot(&self) -> anth_wire::AccountSnapshot {
        crate::accounts::SystemAccounts::snapshot(self)
    }
    fn create_user(&self, request: &anth_wire::AccountCreate) -> Result<String, WireError> {
        crate::accounts::SystemAccounts::create_user(self, request)
    }
    fn delete_user(&self, request: &anth_wire::AccountDelete) -> Result<String, WireError> {
        crate::accounts::SystemAccounts::delete_user(self, request)
    }
    fn set_password(&self, request: &anth_wire::AccountSetPassword) -> Result<String, WireError> {
        crate::accounts::SystemAccounts::set_password(self, request)
    }
    fn create_group(&self, request: &anth_wire::AccountGroupCreate) -> Result<String, WireError> {
        crate::accounts::SystemAccounts::create_group(self, request)
    }
    fn delete_group(&self, request: &anth_wire::AccountGroupDelete) -> Result<String, WireError> {
        crate::accounts::SystemAccounts::delete_group(self, request)
    }
}

/// Every host service one request may need.
///
/// The real build holds one set for the whole daemon; the demo build holds one
/// set per anonymous session. `files` keeps its construction error because a
/// missing or unreadable root must fail that feature without taking the other
/// services down.
#[derive(Clone)]
pub struct Backends {
    pub files: Result<std::sync::Arc<dyn FilesService>, String>,
    pub terminal: std::sync::Arc<dyn TerminalService>,
    pub process: std::sync::Arc<dyn ProcessesService>,
    pub network: std::sync::Arc<dyn NetworkService>,
    pub firewall: std::sync::Arc<dyn FirewallService>,
    pub volumes: std::sync::Arc<dyn VolumesService>,
    pub accounts: std::sync::Arc<dyn AccountsService>,
}
