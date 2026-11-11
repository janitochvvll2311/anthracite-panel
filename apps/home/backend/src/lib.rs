//! Shared boot for the Home service binary.
//!
//! `home-service` answers with real host facts. The demo instance answers the
//! same contract from the shared `demo/emulator` fixture, so only the packaged
//! binary changes; the wire contract and the frontend are identical.

use std::sync::Arc;
use std::time::Duration;

use anth_contract::Error;
use anth_service::{ChunkStream, Handler};
use serde_json::Value;

/// One host snapshot every `interval-ms` (clamped to 1–60 s), encoded as the
/// JSON chunks the engine frames as SSE (§20). The snapshot function runs on the
/// blocking pool: reading `/proc` or running `df` must not stall the runtime.
pub fn watch<F>(input: &Value, snapshot: F) -> Result<ChunkStream, Error>
where
    F: Fn() -> Value + Send + Sync + 'static,
{
    let milliseconds = match input.get("interval-ms") {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
    .unwrap_or(2_000)
    .clamp(1_000, 60_000);
    let interval = Duration::from_millis(milliseconds);
    let snapshot = Arc::new(snapshot);
    Ok(Box::pin(futures_util::stream::unfold(
        (true, interval, snapshot),
        |(first, interval, snapshot)| async move {
            if !first {
                tokio::time::sleep(interval).await;
            }
            let call = Arc::clone(&snapshot);
            let value = tokio::task::spawn_blocking(move || call()).await;
            let result = match value {
                Ok(value) => serde_json::to_vec(&value)
                    .map_err(|error| Error::internal(format!("encoding host info: {error}"))),
                Err(error) => Err(Error::internal(format!(
                    "the host snapshot task failed: {error}"
                ))),
            };
            Some((result, (false, interval, snapshot)))
        },
    )))
}

/// Serves `handler` on the `--socket` the supervisor passed.
pub fn serve(handler: Arc<dyn Handler>) {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = runtime.block_on(anth_service::serve(&socket, handler)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
