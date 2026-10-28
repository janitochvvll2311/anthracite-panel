//! Account inventory and guarded mutations.
//!
//! The inventory merges the local `passwd`/`group` files with what `getent`
//! resolves, and reads `/etc/shadow` for one derived bit: whether an account is
//! locked. Mutations are planned against a fresh, unfitted snapshot and run as
//! direct argv vectors; a password only ever travels on `chpasswd`'s stdin.

mod inventory;
mod mutate;

use std::sync::Arc;

use anth_wire::{
    AccountCreate, AccountDelete, AccountGroupCreate, AccountGroupDelete, AccountSetPassword,
    AccountSnapshot, WireError,
};

use crate::config::AccountsConfig;

pub use inventory::{HostProbe, LocalFiles};
pub use mutate::{
    AccountOutput, AccountRunner, Guard, PlannedCall, SystemAccountRunner, allowed_shells,
    daemon_user, validate_gecos, validate_home, validate_name, validate_shell,
};

/// The account service, built once from the policy the daemon read at startup.
#[derive(Clone)]
pub struct AccountsService {
    config: AccountsConfig,
    probe: Arc<dyn HostProbe>,
    runner: Arc<dyn AccountRunner>,
    daemon_user: String,
}

impl AccountsService {
    pub fn new(config: AccountsConfig) -> Self {
        Self {
            config,
            probe: Arc::new(inventory::SystemProbe::new()),
            runner: Arc::new(SystemAccountRunner),
            daemon_user: daemon_user(),
        }
    }

    /// Replaces the host reader. Tests use it to script `/etc` without touching
    /// the machine.
    pub fn with_probe(mut self, probe: Arc<dyn HostProbe>) -> Self {
        self.probe = probe;
        self
    }

    /// Replaces the tool runner. Tests assert the exact argv without executing
    /// `useradd` or `groupdel`.
    pub fn with_runner(mut self, runner: Arc<dyn AccountRunner>) -> Self {
        self.runner = runner;
        self
    }

    /// Overrides the account the self-protection guard refuses to delete.
    pub fn with_daemon_user(mut self, name: impl Into<String>) -> Self {
        self.daemon_user = name.into();
        self
    }

    pub fn snapshot(&self) -> AccountSnapshot {
        inventory::collect(&self.config, self.probe.as_ref()).0
    }

    pub fn create_user(&self, request: &AccountCreate) -> Result<String, WireError> {
        let call = self.plan(|guard| mutate::plan_create_user(guard, request))?;
        mutate::run_planned(self.runner.as_ref(), &call)?;
        Ok(format!("created user {}", request.name))
    }

    pub fn delete_user(&self, request: &AccountDelete) -> Result<String, WireError> {
        let call = self.plan(|guard| mutate::plan_delete_user(guard, request))?;
        mutate::run_planned(self.runner.as_ref(), &call)?;
        Ok(format!("deleted user {}", request.name))
    }

    pub fn set_password(&self, request: &AccountSetPassword) -> Result<String, WireError> {
        let call = self.plan(|guard| mutate::plan_set_password(guard, request))?;
        mutate::run_planned(self.runner.as_ref(), &call)?;
        Ok(format!("set the password for {}", request.name))
    }

    pub fn create_group(&self, request: &AccountGroupCreate) -> Result<String, WireError> {
        let call = self.plan(|guard| mutate::plan_create_group(guard, request))?;
        mutate::run_planned(self.runner.as_ref(), &call)?;
        Ok(format!("created group {}", request.name))
    }

    pub fn delete_group(&self, request: &AccountGroupDelete) -> Result<String, WireError> {
        let call = self.plan(|guard| mutate::plan_delete_group(guard, request))?;
        mutate::run_planned(self.runner.as_ref(), &call)?;
        Ok(format!("deleted group {}", request.name))
    }

    /// Reads the host once and plans against that same view. The snapshot lives
    /// for the whole closure, so the guard cannot disagree with the plan about
    /// which entries exist or which ids are taken.
    fn plan<T>(
        &self,
        build: impl FnOnce(&Guard<'_>) -> Result<T, WireError>,
    ) -> Result<T, WireError> {
        let (snapshot, _warnings, ranges) = inventory::collect(&self.config, self.probe.as_ref());
        let guard = Guard {
            snapshot: &snapshot,
            config: &self.config,
            daemon_user: self.daemon_user.clone(),
            id_ranges: ranges,
        };
        build(&guard)
    }
}
