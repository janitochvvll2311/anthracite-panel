//! What the daemon does with a request.
//!
//! v0.1.0 required a new operation to appear in two protocol enums plus three
//! hand-written `match` arms (50 in `handler.rs`, 43 in `client.rs`, 9 in
//! `daemon.rs`), with nothing tying them together. Here there is one `match`:
//! the socket loop, the CLI client and the HTTP route all call [`handle`], so a
//! new operation is one arm rather than a variant repeated across three files.

use anth_wire::{Request, Response, WireError};

/// Handles one request.
///
/// A feature adds an arm to [`dispatch`] and nowhere else: the socket loop, the
/// CLI client and the HTTP route all reach the daemon through this function.
pub async fn handle(request: Request) -> Response {
    match dispatch(request).await {
        Ok(response) => response,
        Err(error) => Response::Error(error),
    }
}

async fn dispatch(request: Request) -> Result<Response, WireError> {
    match request {
        Request::Hello { protocol_version } => hello(protocol_version),
        Request::HostInfo => Ok(Response::HostInfo(anth_host::collect())),
    }
}

fn hello(protocol_version: u16) -> Result<Response, WireError> {
    if protocol_version != anth_wire::PROTOCOL_VERSION {
        return Err(WireError::invalid(format!(
            "this daemon speaks protocol {}, not {protocol_version}",
            anth_wire::PROTOCOL_VERSION
        )));
    }
    Ok(Response::Welcome {
        protocol_version: anth_wire::PROTOCOL_VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_wire::PROTOCOL_VERSION;

    #[tokio::test]
    async fn hello_at_the_current_version_is_welcomed() {
        let response = handle(Request::Hello {
            protocol_version: PROTOCOL_VERSION,
        })
        .await;
        assert_eq!(
            response,
            Response::Welcome {
                protocol_version: PROTOCOL_VERSION
            }
        );
    }

    #[tokio::test]
    async fn hello_at_another_version_is_refused_as_invalid() {
        let response = handle(Request::Hello {
            protocol_version: PROTOCOL_VERSION + 1,
        })
        .await;
        let error = response
            .error()
            .expect("a version mismatch must be an error");
        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
        assert!(
            error.message.contains(&PROTOCOL_VERSION.to_string()),
            "the message must name the version this daemon speaks: {}",
            error.message
        );
    }
}
