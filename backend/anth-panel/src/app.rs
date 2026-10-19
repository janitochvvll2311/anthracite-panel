use std::{error::Error, path::Path};

use crate::config::Config;
use anth_proto::AnthClient;
use tokio::net::UnixStream;

#[derive(Clone, Debug)]
pub struct AppState {
    pub config: Config,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// Opens one connection to the daemon. Callers that need independent read
    /// and write halves (the terminal bridge) use this directly.
    pub async fn connect_stream(&self) -> Result<UnixStream, Box<dyn Error + Send + Sync>> {
        let socket_path = Path::new(&self.config.socket_path);
        match UnixStream::connect(socket_path).await {
            Ok(stream) => Ok(stream),
            Err(e) => {
                eprintln!("Failed to connect to socket: {e}");
                Err(Box::new(e))
            }
        }
    }

    pub async fn create_client(&self) -> Result<AnthClient, Box<dyn Error + Send + Sync>> {
        Ok(AnthClient::new(self.connect_stream().await?))
    }
}
