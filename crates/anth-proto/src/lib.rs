use std::error::Error;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::net::UnixStream;
use tokio_util::codec::{Framed, LinesCodec};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "payload")]
pub enum Request {
    Echo { message: String },
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
#[serde(tag = "status", content = "data")]
pub enum Response {
    Ok { message: String },
    Error { detail: String },
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
