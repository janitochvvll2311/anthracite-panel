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
        serde_json::to_string(self).map_err(|e| Box::new(e).into())
    }

    pub fn from_json(json: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        serde_json::from_str(json).map_err(|e| Box::new(e).into())
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
        serde_json::to_string(self).map_err(|e| Box::new(e).into())
    }

    pub fn from_json(json: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        serde_json::from_str(json).map_err(|e| Box::new(e).into())
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

    pub async fn receive_request(&mut self) -> Result<Request, Box<dyn Error + Send + Sync>> {
        if let Some(line) = self.framed.next().await {
            let json = line?;
            let request = Request::from_json(&json)?;
            Ok(request)
        } else {
            Err("Stream closed".into())
        }
    }

    pub async fn send_response(
        &mut self,
        response: &Response,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let json = response.to_json()?;
        self.framed.send(json).await?;
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
        self.framed.send(json).await?;
        Ok(())
    }

    pub async fn receive_response(&mut self) -> Result<Response, Box<dyn Error + Send + Sync>> {
        if let Some(line) = self.framed.next().await {
            let json = line?;
            let response = Response::from_json(&json)?;
            Ok(response)
        } else {
            Err("Stream closed".into())
        }
    }
}
