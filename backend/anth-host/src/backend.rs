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
