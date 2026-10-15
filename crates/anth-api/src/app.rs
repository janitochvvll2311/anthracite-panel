use std::{error::Error, path::Path};

use anth_conf::Config;
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

    pub async fn create_client(&self) -> Result<AnthClient, Box<dyn Error + Send + Sync>> {
        let socket_path = Path::new(&self.config.socket_path);
        let socket = match UnixStream::connect(&socket_path).await {
            Ok(stream) => stream,
            Err(e) => {
                eprintln!("Failed to connect to socket: {e}");
                return Err(Box::new(e));
            }
        };

        Ok(AnthClient::new(socket))
    }
}
