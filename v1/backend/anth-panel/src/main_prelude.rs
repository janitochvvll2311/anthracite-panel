//! Imports shared by every `cli_dispatch` module.
//!
//! The binary is one crate, so a module that needs one more name imports it
//! directly; this list only stops every module from repeating the same thirty
//! `use` lines.

#![allow(unused_imports)]

pub use std::cell::RefCell;
pub use std::io::{Read, Write};
pub use std::net::SocketAddr;
pub use std::path::Path;
pub use std::process::ExitCode;
pub use std::rc::Rc;
pub use std::time::Duration;

pub use clap::Parser;
pub use tokio::io::AsyncBufReadExt;
pub use tokio::net::TcpListener;

pub use anth_panel::cli::{
    AccountArgs, AccountCommand, AccountGroupCommand, AccountGroupDeleteArgs,
    AccountGroupModifyArgs, AccountUserArgs, AccountUserCommand, AccountUserDeleteArgs,
    AccountUserPasswordArgs, AuthArgs, AuthCommand, Cli, Command, ConfigArgs, ConfigCommand,
    EndpointArgs, FileArgs, FileCommand, FileDeleteArgs, FileReadArgs, FileUploadArgs,
    FileWriteArgs, FirewallAddArgs, FirewallArgs, FirewallCommand, FirewallDeleteArgs,
    FirewallFlushArgs, NetworkArgs, NetworkCommand, NetworkKindArg, NetworkStateArgs,
    PartitionCommand, PartitionCreateCli, PartitionDeleteCli, PartitionFormatCli,
    PartitionResizeCli, PluginArgs, PluginCommand, ProcessArgs, ProcessCommand, ProcessSignalArgs,
    ProcessSpawnArgs, TrashBinCommand, TrashEmptyArgs, TrashRestoreArgs, VolumeArgs, VolumeCommand,
    VolumeMountArgs, VolumePartitionsArgs, VolumeUnmountArgs, WatchArgs,
};
pub use anth_panel::config::{Config, DEFAULT_CONFIG_PATH};
pub use anth_panel::read_password;
pub use anth_panel::stream;
pub use anth_panel::{AppState, request_for, router};
pub use anth_proto::{
    AnthClient, DEFAULT_FILE_CHUNK, FileChunk, FirewallAction, Request, Response,
};
