//! The one place that decides what a request does.
//!
//! The socket and HTTP transports only translate. Every command reaches
//! [`Daemon::execute`], so a transport cannot answer a request differently.

use std::sync::Arc;
use std::time::Duration;

use anth_host::backend::{
    FilesService, FirewallService, NetworkService, ProcessesService, TerminalService,
    VolumesService,
};
use anth_wire::{
    ErrorKind, FileBytes, MAX_TERMINAL_INPUT, PROTOCOL_VERSION, PageRequest, Request, Response,
    TerminalOpened, TerminalOutput, TerminalSize, WireError,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use crate::accounts::AccountsService;
use crate::config::{Config, Paths};
use crate::firewall::SystemFirewall;
use crate::lifecycle::{ACTION_GRACE, Lifecycle};
use crate::log::{Level, Logger};
use crate::network::SystemNetwork;
use crate::plugins::PluginService;
use anth_host::files::SystemFiles;
use anth_host::process::SystemProcesses;
use anth_host::terminal::{OpenError, SystemTerminal, resolve_shell, supports_line_editing};
use anth_host::volumes::SystemVolumes;

/// Everything a request may need from the process.
pub struct Daemon {
    config: Config,
    paths: Paths,
    logger: Arc<Logger>,
    lifecycle: Arc<Lifecycle>,
    files: Result<Arc<dyn FilesService>, String>,
    terminal: Arc<dyn TerminalService>,
    process: Arc<dyn ProcessesService>,
    network: Arc<dyn NetworkService>,
    firewall: Arc<dyn FirewallService>,
    volumes: Arc<dyn VolumesService>,
    accounts: AccountsService,
    plugins: PluginService,
}

impl Daemon {
    /// A daemon that may restart but never reboot.
    pub fn new(config: Config, paths: Paths, logger: Arc<Logger>) -> Self {
        Self::with_lifecycle(config, paths, logger, Arc::new(Lifecycle::without_reboot()))
    }

    pub fn with_lifecycle(
        config: Config,
        paths: Paths,
        logger: Arc<Logger>,
        lifecycle: Arc<Lifecycle>,
    ) -> Self {
        let demo = std::env::var("ANTH_BACKEND")
            .map(|value| value.eq_ignore_ascii_case("demo"))
            .unwrap_or(false);
        let root = if demo {
            paths.state_dir.join("demo/files")
        } else {
            let root = std::path::PathBuf::from(&config.files.root);
            if root.is_absolute() {
                root
            } else {
                paths.config_dir.join(root)
            }
        };
        let files_root = root.clone();
        let files: Result<Arc<dyn FilesService>, String> = if demo {
            anth_host::demo::DemoFiles::new(
                root,
                config.files.max_entries,
                config.files.max_preview_bytes,
                config.files.max_upload_bytes,
                config.files.max_trash_bytes,
                config.files.max_archive_bytes,
            )
            .map(|service| Arc::new(service) as Arc<dyn FilesService>)
        } else {
            SystemFiles::new(
                root,
                config.files.write,
                config.files.max_entries,
                config.files.max_preview_bytes,
                config.files.max_upload_bytes,
            )
            .map(|service| {
                Arc::new(
                    service
                        .with_trash(config.files.trash, config.files.max_trash_bytes)
                        .with_archive_limit(config.files.max_archive_bytes),
                ) as Arc<dyn FilesService>
            })
        };
        let terminal: Arc<dyn TerminalService> = Arc::new(SystemTerminal::new(
            config.terminal.enabled,
            config.terminal.max_sessions,
            Duration::from_secs(config.terminal.idle_seconds),
        ));
        let process: Arc<dyn ProcessesService> = if demo {
            Arc::new(anth_host::demo::DemoProcesses::new(
                config.process.control,
                config.process.spawn,
                config.process.spawn_allow.clone(),
            ))
        } else {
            Arc::new(SystemProcesses::new(
                config.process.control,
                config.process.spawn,
                config.process.spawn_allow.clone(),
            ))
        };
        let network: Arc<dyn NetworkService> = if demo {
            Arc::new(anth_host::demo::DemoNetwork::new(
                config.network.control,
                config.network.manage,
            ))
        } else {
            Arc::new(SystemNetwork::new(
                config.network.control,
                config.network.manage,
            ))
        };
        let firewall: Arc<dyn FirewallService> = if demo {
            Arc::new(anth_host::demo::DemoFirewall::new(config.firewall.control))
        } else {
            Arc::new(SystemFirewall::new(config.firewall.control))
        };
        let mut protected = vec![
            files_root,
            paths.config.clone(),
            paths.socket.clone(),
            paths.state_dir.clone(),
            paths.secrets.clone(),
            paths.sessions.clone(),
            paths.plugins.clone(),
            paths.grants.clone(),
            paths.plugin_state.clone(),
        ];
        if let Ok(executable) = std::env::current_exe() {
            protected.push(executable);
        }
        let volumes: Arc<dyn VolumesService> = if demo {
            Arc::new(anth_host::demo::DemoVolumes::new(config.volumes.settings()))
        } else {
            Arc::new(SystemVolumes::new(config.volumes.settings(), protected))
        };
        let accounts = AccountsService::new(config.accounts.clone());
        let plugins = PluginService::new(
            config.plugins.clone(),
            paths.plugins.clone(),
            paths.grants.clone(),
        );
        Self {
            config,
            paths,
            logger,
            lifecycle,
            files,
            terminal,
            process,
            network,
            firewall,
            volumes,
            accounts,
            plugins,
        }
    }

    /// Replaces the network service. Tests use it to script `ip` without
    /// touching the host; the transport still goes through `execute`.
    pub fn with_network(mut self, network: SystemNetwork) -> Self {
        self.network = Arc::new(network);
        self
    }

    /// Replaces the network backend with any implementation.
    pub fn with_network_backend(mut self, network: Arc<dyn NetworkService>) -> Self {
        self.network = network;
        self
    }

    /// Replaces the firewall service. Tests use it to script `nft` without
    /// touching the host.
    pub fn with_firewall(mut self, firewall: SystemFirewall) -> Self {
        self.firewall = Arc::new(firewall);
        self
    }

    /// Replaces the firewall backend with any implementation.
    pub fn with_firewall_backend(mut self, firewall: Arc<dyn FirewallService>) -> Self {
        self.firewall = firewall;
        self
    }

    /// Replaces the volumes service. Tests use it to script the probe without
    /// touching the host.
    pub fn with_volumes(mut self, volumes: SystemVolumes) -> Self {
        self.volumes = Arc::new(volumes);
        self
    }

    /// Replaces the accounts service. Tests use it to script `/etc` and the
    /// account tools without touching the host.
    pub fn with_accounts(mut self, accounts: AccountsService) -> Self {
        self.accounts = accounts;
        self
    }

    /// Replaces the plugin service. Tests use it to script a plugin root
    /// without touching the installation directory.
    pub fn with_plugins(mut self, plugins: PluginService) -> Self {
        self.plugins = plugins;
        self
    }

    pub fn plugins(&self) -> &PluginService {
        &self.plugins
    }

    pub fn terminals(&self) -> &Arc<dyn TerminalService> {
        &self.terminal
    }

    pub fn close_terminals(&self, owner: &str) -> usize {
        self.terminal.close_owner(owner)
    }

    /// Reaps idle terminals forever. Called once by the serving command.
    pub fn spawn_terminal_reaper(self: &Arc<Self>) {
        let daemon = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                daemon.terminal.reap();
            }
        });
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn logger(&self) -> &Arc<Logger> {
        &self.logger
    }

    pub fn lifecycle(&self) -> &Arc<Lifecycle> {
        &self.lifecycle
    }

    fn files(&self) -> Result<&Arc<dyn FilesService>, WireError> {
        self.files
            .as_ref()
            .map_err(|error| WireError::internal(format!("the file root is unavailable: {error}")))
    }

    /// Reads an image for the HTTP preview route.
    ///
    /// Images are a binary resource rather than a wire command: the socket
    /// remains a JSON transport, and the HTTP adapter streams the bytes.
    pub fn image(&self, path: &str) -> Result<(Vec<u8>, &'static str), WireError> {
        self.files()?.image(path)
    }

    /// Reads a file for the HTTP download route.
    pub fn download(&self, path: &str) -> Result<Vec<u8>, WireError> {
        self.files()?.download(path)
    }

    /// Handles one request.
    pub async fn execute(&self, request: Request) -> Response {
        self.execute_as("daemon", request).await
    }

    /// Handles one request for a named owner.
    ///
    /// The socket passes its peer identity; the browser bridge passes the
    /// authenticated session id, so one panel session cannot touch another's
    /// terminal handle.
    pub async fn execute_as(&self, owner: &str, request: Request) -> Response {
        let operation = operation_of(&request);
        let started = std::time::Instant::now();
        let response = match self.dispatch(owner, request).await {
            Ok(response) => response,
            Err(error) => Response::Error(error),
        };
        let failed = response.error().is_some();
        let ms = started.elapsed().as_millis().to_string();
        let level = if failed {
            Level::Warn
        } else if operation == "terminal-read" {
            Level::Debug
        } else {
            Level::Info
        };
        self.logger.event(
            level,
            "request",
            if failed { "failed" } else { "answered" },
            &[("op", operation), ("ms", &ms)],
        );
        response
    }

    async fn dispatch(&self, owner: &str, request: Request) -> Result<Response, WireError> {
        if matches!(
            &request,
            Request::TerminalOpen { .. }
                | Request::TerminalRead { .. }
                | Request::TerminalWrite { .. }
                | Request::TerminalResize { .. }
                | Request::TerminalClose { .. }
        ) && !self.terminal.enabled()
        {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "terminal_disabled",
                "the terminal is disabled; set `[terminal] enabled = true`",
            ));
        }
        match request {
            Request::Hello {
                protocol_version,
                client,
                features,
            } => hello(protocol_version, &client, &features),
            Request::HostInfo => {
                let info = tokio::task::spawn_blocking(anth_host::collect)
                    .await
                    .map_err(|error| {
                        WireError::internal(format!("the host-info task failed: {error}"))
                    })?;
                Ok(Response::HostInfo(info))
            }
            Request::Network => {
                let network = self.network.clone();
                Ok(Response::Network(
                    blocking(move || network.snapshot()).await?,
                ))
            }
            Request::NetworkSetState { name, up } => {
                let network = self.network.clone();
                let message = blocking(move || network.set_state(&name, up)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkCreate(args) => {
                let network = self.network.clone();
                let message = blocking(move || network.create(&args)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkEdit(args) => {
                let network = self.network.clone();
                let message = blocking(move || network.edit(&args)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkRename { name, new_name } => {
                let network = self.network.clone();
                let message = blocking(move || network.rename(&name, &new_name)).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::NetworkDelete { name, peer } => {
                let network = self.network.clone();
                let message = blocking(move || network.delete(&name, peer.as_deref())).await?;
                Ok(Response::NetworkChanged { message })
            }
            Request::FirewallStatus => {
                let firewall = self.firewall.clone();
                Ok(Response::FirewallSnapshot(
                    blocking(move || firewall.snapshot()).await?,
                ))
            }
            Request::FirewallAdd {
                chain,
                action,
                protocol,
                port,
                comment,
            } => {
                let firewall = self.firewall.clone();
                let message = blocking(move || {
                    firewall.add(chain, action, protocol, port, comment.as_deref())
                })
                .await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallUpdate {
                handle,
                chain,
                action,
                protocol,
                port,
                comment,
            } => {
                let firewall = self.firewall.clone();
                let message = blocking(move || {
                    firewall.update(handle, chain, action, protocol, port, comment.as_deref())
                })
                .await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallDelete { handle } => {
                let firewall = self.firewall.clone();
                let message = blocking(move || firewall.delete(handle)).await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallFlush { chain } => {
                let firewall = self.firewall.clone();
                let message = blocking(move || firewall.flush(chain)).await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::FirewallSetPolicy { chain, policy } => {
                let firewall = self.firewall.clone();
                let message = blocking(move || firewall.set_policy(chain, policy)).await?;
                Ok(Response::FirewallChanged { message })
            }
            Request::Volumes => {
                let volumes = self.volumes.clone();
                Ok(Response::Volumes(
                    blocking(move || Ok::<_, WireError>(volumes.snapshot())).await?,
                ))
            }
            Request::VolumesMount(request) => {
                let volumes = self.volumes.clone();
                let message = blocking(move || volumes.mount(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumesUnmount(request) => {
                let volumes = self.volumes.clone();
                let message = blocking(move || volumes.unmount(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumePartitionCreate(request) => {
                let volumes = self.volumes.clone();
                let message = blocking(move || volumes.create_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumePartitionResize(request) => {
                let volumes = self.volumes.clone();
                let message = blocking(move || volumes.resize_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumePartitionDelete(request) => {
                let volumes = self.volumes.clone();
                let message = blocking(move || volumes.delete_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::VolumeFormat(request) => {
                let volumes = self.volumes.clone();
                let message = blocking(move || volumes.format_partition(&request)).await?;
                Ok(Response::VolumeChanged { message })
            }
            Request::Accounts => {
                let accounts = self.accounts.clone();
                Ok(Response::Accounts(
                    blocking(move || Ok::<_, WireError>(accounts.snapshot())).await?,
                ))
            }
            Request::AccountCreate(request) => {
                let accounts = self.accounts.clone();
                let message = blocking(move || accounts.create_user(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountDelete(request) => {
                let accounts = self.accounts.clone();
                let message = blocking(move || accounts.delete_user(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountSetPassword(request) => {
                let accounts = self.accounts.clone();
                let message = blocking(move || accounts.set_password(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountGroupCreate(request) => {
                let accounts = self.accounts.clone();
                let message = blocking(move || accounts.create_group(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::AccountGroupDelete(request) => {
                let accounts = self.accounts.clone();
                let message = blocking(move || accounts.delete_group(&request)).await?;
                Ok(Response::AccountChanged { message })
            }
            Request::Restart => {
                let lifecycle = Arc::clone(&self.lifecycle);
                tokio::spawn(async move {
                    tokio::time::sleep(ACTION_GRACE).await;
                    lifecycle.restart();
                });
                Ok(Response::Restarting {
                    pid: std::process::id(),
                })
            }
            Request::SystemReboot { delay_s } => {
                self.lifecycle.authorize_reboot(delay_s)?;
                let lifecycle = Arc::clone(&self.lifecycle);
                tokio::spawn(async move {
                    tokio::time::sleep(ACTION_GRACE).await;
                    if let Err(error) = lifecycle.perform_reboot(delay_s) {
                        eprintln!("anth-panel: reboot failed: {error}");
                    }
                });
                Ok(Response::Rebooting { delay_s })
            }
            Request::FilesList {
                path,
                limit,
                cursor,
            } => {
                let request = PageRequest { limit, cursor };
                Ok(Response::FilesList(self.files()?.list(&path, &request)?))
            }
            Request::FilesRead {
                path,
                offset,
                max_bytes,
            } => Ok(Response::FilesRead(
                self.files()?.read(&path, offset, max_bytes)?,
            )),
            Request::FilesDownload {
                path,
                offset,
                max_bytes,
            } => {
                let chunk = self.files()?.download_chunk(&path, offset, max_bytes)?;
                Ok(Response::FilesDownload(FileBytes {
                    path: chunk.path,
                    offset: chunk.offset,
                    size: chunk.size,
                    eof: chunk.eof,
                    data_base64: STANDARD.encode(chunk.bytes),
                }))
            }
            Request::FilesDelete { path } => Ok(Response::FilesDeleted {
                path: self.files()?.delete(&path)?,
            }),
            Request::FilesWrite {
                path,
                content,
                create,
            } => Ok(Response::FilesWritten(
                self.files()?.write(&path, &content, create)?,
            )),
            Request::FilesUpload {
                path,
                content_base64,
                create,
            } => {
                let bytes = STANDARD
                    .decode(content_base64.as_bytes())
                    .map_err(|_| WireError::invalid("the upload content is not valid base64"))?;
                Ok(Response::FilesWritten(
                    self.files()?.write_bytes(&path, &bytes, create)?,
                ))
            }
            Request::FilesTrash { path } => {
                let (path, id) = self.files()?.trash(&path)?;
                Ok(Response::FilesTrashed { path, id })
            }
            Request::FilesTrashList => Ok(Response::FilesTrashList(self.files()?.trash_listing()?)),
            Request::FilesTrashRestore { id, overwrite } => {
                let (id, path) = self.files()?.restore(&id, overwrite)?;
                Ok(Response::FilesRestored { id, path })
            }
            Request::FilesTrashEmpty => Ok(Response::FilesTrashEmptied {
                removed: self.files()?.empty_trash()?,
            }),
            Request::FilesArchive {
                path,
                destination,
                format,
            } => Ok(Response::FilesArchiveCreated {
                path: self
                    .files()?
                    .create_archive(&path, destination.as_deref(), format)?,
            }),
            Request::FilesArchiveList { path } => Ok(Response::FilesArchiveList(
                self.files()?.list_archive(&path)?,
            )),
            Request::FilesArchiveExtract { path, into } => {
                let (path, entries) = self.files()?.extract_archive(&path, into.as_deref())?;
                Ok(Response::FilesArchiveExtracted { path, entries })
            }
            Request::TerminalOpen { cols, rows } => {
                let program = resolve_shell(&self.config.terminal.shell).map_err(|detail| {
                    WireError::new(ErrorKind::Forbidden, "terminal_shell", detail)
                })?;
                let size = TerminalSize { cols, rows };
                let line_editing = supports_line_editing(&program);
                let sessions = Arc::clone(&self.terminal);
                let owner = owner.to_string();
                let asked = program.clone();
                let id = blocking(move || {
                    sessions
                        .open(&owner, &asked, size)
                        .map_err(OpenError::into_wire)
                })
                .await?;
                Ok(Response::TerminalOpened(TerminalOpened {
                    id,
                    program,
                    cols,
                    rows,
                    line_editing,
                }))
            }
            Request::TerminalRead { id, wait_ms } => {
                let chunk = self.terminal.read(owner, &id, wait_ms).await?;
                Ok(Response::TerminalOutput(TerminalOutput {
                    id,
                    seq: chunk.seq,
                    bytes_base64: STANDARD.encode(chunk.bytes),
                    eof: chunk.eof,
                    exit_code: chunk.exit_code,
                }))
            }
            Request::TerminalWrite { id, bytes_base64 } => {
                let bytes = STANDARD
                    .decode(bytes_base64.as_bytes())
                    .map_err(|_| WireError::invalid("the terminal input is not valid base64"))?;
                if bytes.len() > MAX_TERMINAL_INPUT {
                    return Err(WireError::too_large(format!(
                        "terminal input is {} bytes, over the {MAX_TERMINAL_INPUT} byte limit",
                        bytes.len()
                    )));
                }
                let sessions = Arc::clone(&self.terminal);
                let owner = owner.to_string();
                blocking(move || sessions.write(&owner, &id, &bytes)).await?;
                Ok(Response::Accepted)
            }
            Request::TerminalResize { id, cols, rows } => {
                self.terminal
                    .resize(owner, &id, TerminalSize { cols, rows })?;
                Ok(Response::Accepted)
            }
            Request::TerminalClose { id } => {
                let sessions = Arc::clone(&self.terminal);
                let owner = owner.to_string();
                blocking(move || sessions.close(&owner, &id)).await?;
                Ok(Response::Accepted)
            }
            Request::Processes { limit, cursor } => {
                let request = PageRequest { limit, cursor };
                let process = self.process.clone();
                let snapshot = blocking(move || process.snapshot(&request)).await?;
                Ok(Response::Processes(snapshot))
            }
            Request::ProcessSignal {
                pid,
                start_ticks,
                signal,
                group,
            } => {
                let process = self.process.clone();
                blocking(move || process.signal(pid, start_ticks, signal, group)).await?;
                Ok(Response::ProcessSignalled { pid, signal })
            }
            Request::ProcessSpawn { program, args, cwd } => {
                let process = self.process.clone();
                let started =
                    blocking(move || process.spawn(&program, &args, cwd.as_deref())).await?;
                Ok(Response::ProcessStarted(started))
            }
        }
    }
}

fn hello(protocol_version: u16, client: &str, features: &[String]) -> Result<Response, WireError> {
    if protocol_version != PROTOCOL_VERSION {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "protocol_mismatch",
            format!(
                "this daemon speaks protocol {PROTOCOL_VERSION}, the client speaks {protocol_version}"
            ),
        ));
    }
    if client.is_empty() || client.len() > 64 || features.len() > 32 {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "invalid_hello",
            "the Hello client or feature list is out of bounds",
        ));
    }
    if features.iter().any(|feature| feature.len() > 64) {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "invalid_hello",
            "a Hello feature is longer than 64 characters",
        ));
    }

    Ok(Response::Welcome {
        protocol_version: PROTOCOL_VERSION,
        server: "anth-panel".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        features: Vec::new(),
    })
}

/// Runs a blocking PTY operation on the blocking pool.
///
/// `portable-pty` spawns processes and reaps them with `waitpid`; doing that on
/// a reactor thread would stall every other request.
async fn blocking<T, F>(task: F) -> Result<T, WireError>
where
    F: FnOnce() -> Result<T, WireError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| WireError::internal(format!("the blocking task failed: {error}")))?
}

fn operation_of(request: &Request) -> &'static str {
    match request {
        Request::Hello { .. } => "hello",
        Request::HostInfo => "host-info",
        Request::Network => "network",
        Request::NetworkSetState { .. } => "network-set-state",
        Request::NetworkCreate(_) => "network-create",
        Request::NetworkEdit(_) => "network-edit",
        Request::NetworkRename { .. } => "network-rename",
        Request::NetworkDelete { .. } => "network-delete",
        Request::FirewallStatus => "firewall-status",
        Request::FirewallAdd { .. } => "firewall-add",
        Request::FirewallUpdate { .. } => "firewall-update",
        Request::FirewallDelete { .. } => "firewall-delete",
        Request::FirewallFlush { .. } => "firewall-flush",
        Request::FirewallSetPolicy { .. } => "firewall-set-policy",
        Request::Volumes => "volumes",
        Request::VolumesMount(_) => "volumes-mount",
        Request::VolumesUnmount(_) => "volumes-unmount",
        Request::VolumePartitionCreate(_) => "volume-partition-create",
        Request::VolumePartitionResize(_) => "volume-partition-resize",
        Request::VolumePartitionDelete(_) => "volume-partition-delete",
        Request::VolumeFormat(_) => "volume-format",
        Request::Accounts => "accounts",
        Request::AccountCreate(_) => "account-create",
        Request::AccountDelete(_) => "account-delete",
        Request::AccountSetPassword(_) => "account-set-password",
        Request::AccountGroupCreate(_) => "account-group-create",
        Request::AccountGroupDelete(_) => "account-group-delete",
        Request::Restart => "restart",
        Request::SystemReboot { .. } => "system-reboot",
        Request::FilesList { .. } => "files-list",
        Request::FilesRead { .. } => "files-read",
        Request::FilesDownload { .. } => "files-download",
        Request::FilesDelete { .. } => "files-delete",
        Request::FilesWrite { .. } => "files-write",
        Request::FilesUpload { .. } => "files-upload",
        Request::FilesTrash { .. } => "files-trash",
        Request::FilesTrashList => "files-trash-list",
        Request::FilesTrashRestore { .. } => "files-trash-restore",
        Request::FilesTrashEmpty => "files-trash-empty",
        Request::FilesArchive { .. } => "files-archive",
        Request::FilesArchiveList { .. } => "files-archive-list",
        Request::FilesArchiveExtract { .. } => "files-archive-extract",
        Request::TerminalOpen { .. } => "terminal-open",
        Request::TerminalRead { .. } => "terminal-read",
        Request::TerminalWrite { .. } => "terminal-write",
        Request::TerminalResize { .. } => "terminal-resize",
        Request::TerminalClose { .. } => "terminal-close",
        Request::Processes { .. } => "processes",
        Request::ProcessSignal { .. } => "process-signal",
        Request::ProcessSpawn { .. } => "process-spawn",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::lifecycle::{Recorded, recorded};
    use crate::log::Settings;

    fn daemon(allow_reboot: bool) -> (Arc<Daemon>, Arc<Recorded>) {
        let config = Config::dev_default();
        let paths = config
            .paths("/tmp/anthracite-panel/tests/dispatch/config.toml")
            .expect("paths");
        let (lifecycle, recorder) = recorded(allow_reboot);
        let logger = Arc::new(Logger::buffer(Settings::default()).0);
        (
            Arc::new(Daemon::with_lifecycle(
                config,
                paths,
                logger,
                Arc::new(lifecycle),
            )),
            recorder,
        )
    }

    #[tokio::test]
    async fn restart_is_answered_before_the_action() {
        let (daemon, recorder) = daemon(false);
        let response = daemon.execute(Request::Restart).await;
        assert!(matches!(response, Response::Restarting { .. }));
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(200)).await;
        assert_eq!(recorder.restarts.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reboot_without_the_switch_is_refused_and_not_scheduled() {
        let (daemon, recorder) = daemon(false);
        let response = daemon.execute(Request::SystemReboot { delay_s: 0 }).await;
        assert_eq!(response.error().expect("error").kind, ErrorKind::Forbidden);
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(100)).await;
        assert!(recorder.reboots.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_authorized_reboot_is_scheduled() {
        let (daemon, recorder) = daemon(true);
        let response = daemon.execute(Request::SystemReboot { delay_s: 7 }).await;
        assert!(matches!(response, Response::Rebooting { delay_s: 7 }));
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(200)).await;
        assert_eq!(recorder.reboots.lock().unwrap().as_slice(), &[7]);
    }
}
