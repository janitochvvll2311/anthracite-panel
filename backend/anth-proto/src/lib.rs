use std::error::Error;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, FramedRead, FramedWrite, LinesCodec};

pub mod b64;
pub mod terminal;

pub use terminal::{
    TerminalOpenArgs, TerminalOpened, TerminalSize, DEFAULT_TERMINAL_SIZE, MAX_TERMINAL_CHUNK,
    MAX_TERMINAL_INPUT,
};

/// Largest line accepted from a peer, in bytes.
///
/// `LinesCodec::new()` uses `usize::MAX`, so a client that opens the socket and
/// never sends `\n` makes the daemon buffer without bound. Host info and echo
/// are far below this; a terminal frame is capped separately.
pub const MAX_LINE_LEN: usize = 1024 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data")]
pub enum Request {
    Echo {
        message: String,
    },
    /// Ask the daemon for a snapshot of the host it runs on.
    HostInfo,
    /// Start a terminal session. The reply is `Response::TerminalOpened`,
    /// after which output arrives unsolicited as `Response::TerminalOutput`.
    TerminalOpen(TerminalOpenArgs),
    /// Bytes typed into the terminal. Opaque: the daemon does not interpret
    /// them, so control sequences pass through untouched.
    TerminalInput {
        id: u32,
        #[serde(with = "b64")]
        bytes: Vec<u8>,
    },
    /// Tell the process group its window changed size.
    TerminalResize {
        id: u32,
        size: TerminalSize,
    },
    /// Close the session and kill its process group.
    TerminalClose {
        id: u32,
    },
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
    /// A session exists. Sent once per `Request::TerminalOpen`.
    TerminalOpened(TerminalOpened),
    /// Terminal output. Unsolicited: the daemon sends these as the session
    /// produces bytes, which is why the connection needs independent halves.
    TerminalOutput {
        id: u32,
        #[serde(with = "b64")]
        bytes: Vec<u8>,
    },
    /// The session's process exited. Terminal; no more output follows.
    TerminalExited {
        id: u32,
        status: i32,
    },
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

impl From<&HostInfo> for serde_json::Value {
    /// The bare snapshot, without the `Response` tag envelope. The REST API
    /// uses this so a route names the command instead of the payload doing it.
    fn from(info: &HostInfo) -> Self {
        serde_json::json!({
            "hostname": info.hostname,
            "os": {
                "name": info.os.name,
                "version": info.os.version,
                "kernel": info.os.kernel,
                "arch": info.os.arch,
            },
            "uptime_seconds": info.uptime_seconds,
            "cpu_model": info.cpu_model,
            "logical_cores": info.logical_cores,
            "load": {
                "one": info.load.one,
                "five": info.load.five,
                "fifteen": info.load.fifteen,
            },
            "memory": {
                "total_kib": info.memory.total_kib,
                "available_kib": info.memory.available_kib,
                "swap_total_kib": info.memory.swap_total_kib,
                "swap_free_kib": info.memory.swap_free_kib,
            },
        })
    }
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

/// Decoder and encoder for the line protocol, with a bound on line length.
///
/// Exposed so a server can drive the two directions independently: terminal
/// output must be written while input is still being read.
pub fn codec() -> LinesCodec {
    LinesCodec::new_with_max_length(MAX_LINE_LEN)
}

/// Receiving half of a connection.
pub struct AnthReader {
    framed: FramedRead<OwnedReadHalf, LinesCodec>,
}

impl AnthReader {
    pub fn new(stream: UnixStream) -> Self {
        let (read, _) = stream.into_split();
        Self::from_read_half(read)
    }

    pub fn from_read_half(read: OwnedReadHalf) -> Self {
        Self {
            framed: FramedRead::new(read, codec()),
        }
    }

    /// Next request, or `None` once the peer disconnects.
    pub async fn receive_request(
        &mut self,
    ) -> Option<Result<Request, Box<dyn Error + Send + Sync>>> {
        match self.framed.next().await {
            Some(Ok(json)) => Some(Request::from_json(&json)),
            Some(Err(e)) => Some(Err(Box::new(e))),
            None => None,
        }
    }

    /// Next response, or `None` once the peer disconnects. Client direction:
    /// pairs with [`AnthWriter::send_request`].
    pub async fn receive_response(
        &mut self,
    ) -> Option<Result<Response, Box<dyn Error + Send + Sync>>> {
        match self.framed.next().await {
            Some(Ok(json)) => Some(Response::from_json(&json)),
            Some(Err(e)) => Some(Err(Box::new(e))),
            None => None,
        }
    }
}

/// Sending half of a connection. Cloning is not needed: hand the one writer to
/// the task that owns outgoing frames.
pub struct AnthWriter {
    framed: FramedWrite<OwnedWriteHalf, LinesCodec>,
}

impl AnthWriter {
    pub fn new(stream: UnixStream) -> Self {
        let (_, write) = stream.into_split();
        Self::from_write_half(write)
    }

    pub fn from_write_half(write: OwnedWriteHalf) -> Self {
        Self {
            framed: FramedWrite::new(write, codec()),
        }
    }

    pub async fn send_response(
        &mut self,
        response: &Response,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let json = response.to_json()?;
        self.framed.send(json).await.map_err(|e| Box::new(e) as _)
    }

    /// Sends a request. Client direction: pairs with
    /// [`AnthReader::receive_response`].
    pub async fn send_request(
        &mut self,
        request: &Request,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let json = request.to_json()?;
        self.framed.send(json).await.map_err(|e| Box::new(e) as _)
    }
}

pub struct AnthServer {
    framed: Framed<UnixStream, LinesCodec>,
}

impl AnthServer {
    pub fn new(stream: UnixStream) -> Self {
        Self {
            framed: Framed::new(stream, codec()),
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
            framed: Framed::new(stream, codec()),
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
