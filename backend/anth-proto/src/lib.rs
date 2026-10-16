use std::error::Error;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LinesCodec};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum Request {
    Echo {
        message: String,
    },
    /// Ask the daemon for a snapshot of the host it runs on.
    HostInfo,
}

impl Request {
    pub fn to_json(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        match serde_json::to_string(self) {
            Ok(json) => Ok(json),
            Err(e) => {
                eprintln!("Error serializing request to JSON: {e}");
                Err(Box::new(e).into())
            }
        }
    }

    pub fn from_json(json: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        match serde_json::from_str(json) {
            Ok(request) => Ok(request),
            Err(e) => {
                eprintln!("Error parsing request from JSON: {e}");
                Err(Box::new(e).into())
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum Response {
    Ok {
        message: String,
    },
    Error {
        detail: String,
    },
    /// Boxed to keep the enum small; the payload is much larger than the others.
    HostInfo(Box<HostInfo>),
}

/// Operating system identity reported by the daemon host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Memory figures in kibibytes, as reported by the kernel.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryInfo {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

/// Snapshot of the host the daemon runs on. Deliberately excludes per-process
/// inspection.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct HostInfo {
    pub hostname: String,
    pub os: OsInfo,
    pub uptime_seconds: u64,
    pub cpu_model: String,
    pub logical_cores: usize,
    pub load: LoadAverage,
    pub memory: MemoryInfo,
}

impl Response {
    pub fn to_json(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        match serde_json::to_string(self) {
            Ok(json) => Ok(json),
            Err(e) => {
                eprintln!("Error serializing response to JSON: {e}");
                Err(Box::new(e).into())
            }
        }
    }

    pub fn from_json(json: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        match serde_json::from_str(json) {
            Ok(response) => Ok(response),
            Err(e) => {
                eprintln!("Error parsing response from JSON: {e}");
                Err(Box::new(e).into())
            }
        }
    }
}

pub struct AnthServer {
    framed: Framed<UnixStream, LinesCodec>,
}

impl AnthServer {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            framed: Framed::new(stream, LinesCodec::new()),
        }
    }

    pub async fn receive_request(
        &mut self,
    ) -> Option<Result<Request, Box<dyn Error + Send + Sync>>> {
        if let Some(line) = self.framed.next().await {
            match line {
                Ok(json) => Some(Request::from_json(&json)),
                Err(e) => {
                    eprintln!("Error reading line from stream: {e}");
                    Some(Err(Box::new(e).into()))
                }
            }
        } else {
            eprintln!("Stream closed while waiting for request");
            None
        }
    }

    pub async fn send_response(
        &mut self,
        response: &Response,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let json = response.to_json()?;
        if let Err(e) = self.framed.send(json).await {
            eprintln!("Error sending response: {e}");
            return Err(Box::new(e).into());
        }
        Ok(())
    }
}

pub struct AnthClient {
    framed: Framed<UnixStream, LinesCodec>,
}

impl AnthClient {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            framed: Framed::new(stream, LinesCodec::new()),
        }
    }

    pub async fn send_request(
        &mut self,
        request: &Request,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let json = request.to_json()?;
        if let Err(e) = self.framed.send(json).await {
            eprintln!("Error sending request: {e}");
            return Err(Box::new(e).into());
        }
        Ok(())
    }

    pub async fn receive_response(
        &mut self,
    ) -> Option<Result<Response, Box<dyn Error + Send + Sync>>> {
        if let Some(line) = self.framed.next().await {
            match line {
                Ok(json) => Some(Response::from_json(&json)),
                Err(e) => {
                    eprintln!("Error reading line from stream: {e}");
                    Some(Err(Box::new(e).into()))
                }
            }
        } else {
            eprintln!("Stream closed while waiting for response");
            None
        }
    }
}
