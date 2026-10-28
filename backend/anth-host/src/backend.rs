//! Backend ports for the host boundary.
//!
//! `anth-panel` talks to these traits. The system implementations in
//! `network`/`firewall` (and the modules added in later phases) perform real
//! host operations; `demo` implements the same contracts in memory.

use anth_wire::{
    FirewallAction, FirewallChain, FirewallProtocol, FirewallSnapshot, NetworkCreate, NetworkEdit,
    NetworkSnapshot, WireError,
};

use crate::firewall::FirewallService;
use crate::network::NetworkService;

pub trait NetworkBackend: Send + Sync {
    fn snapshot(&self) -> Result<NetworkSnapshot, WireError>;
    fn set_state(&self, name: &str, up: bool) -> Result<String, WireError>;
    fn create(&self, args: &NetworkCreate) -> Result<String, WireError>;
    fn edit(&self, args: &NetworkEdit) -> Result<String, WireError>;
    fn rename(&self, name: &str, new_name: &str) -> Result<String, WireError>;
    fn delete(&self, name: &str, peer: Option<&str>) -> Result<String, WireError>;
}

pub trait FirewallBackend: Send + Sync {
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

impl NetworkBackend for NetworkService {
    fn snapshot(&self) -> Result<NetworkSnapshot, WireError> {
        NetworkService::snapshot(self)
    }
    fn set_state(&self, name: &str, up: bool) -> Result<String, WireError> {
        NetworkService::set_state(self, name, up)
    }
    fn create(&self, args: &NetworkCreate) -> Result<String, WireError> {
        NetworkService::create(self, args)
    }
    fn edit(&self, args: &NetworkEdit) -> Result<String, WireError> {
        NetworkService::edit(self, args)
    }
    fn rename(&self, name: &str, new_name: &str) -> Result<String, WireError> {
        NetworkService::rename(self, name, new_name)
    }
    fn delete(&self, name: &str, peer: Option<&str>) -> Result<String, WireError> {
        NetworkService::delete(self, name, peer)
    }
}

impl FirewallBackend for FirewallService {
    fn snapshot(&self) -> Result<FirewallSnapshot, WireError> {
        FirewallService::snapshot(self)
    }
    fn add(
        &self,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        FirewallService::add(self, chain, action, protocol, port, comment)
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
        FirewallService::update(self, handle, chain, action, protocol, port, comment)
    }
    fn delete(&self, handle: u64) -> Result<String, WireError> {
        FirewallService::delete(self, handle)
    }
    fn flush(&self, chain: FirewallChain) -> Result<String, WireError> {
        FirewallService::flush(self, chain)
    }
    fn set_policy(
        &self,
        chain: FirewallChain,
        policy: FirewallAction,
    ) -> Result<String, WireError> {
        FirewallService::set_policy(self, chain, policy)
    }
}
