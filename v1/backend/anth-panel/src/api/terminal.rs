use super::prelude::*;

/// Query for `GET /api/terminal`. Both members are optional together; the
/// browser normally sends both from the fitted xterm size.
#[derive(Debug, Deserialize)]
pub struct TerminalQuery {
    cols: Option<u16>,
    rows: Option<u16>,
}

fn requested_terminal_size(query: TerminalQuery) -> Result<TerminalSize, String> {
    let size = match (query.cols, query.rows) {
        (None, None) => DEFAULT_TERMINAL_SIZE,
        (Some(cols), Some(rows)) => TerminalSize { cols, rows },
        _ => return Err("cols and rows must be provided together".to_string()),
    };

    if !size.is_sane() {
        return Err(format!(
            "terminal size {}x{} is outside {}x{}..{}x{}",
            size.cols,
            size.rows,
            TerminalSize::MIN.cols,
            TerminalSize::MIN.rows,
            TerminalSize::MAX.cols,
            TerminalSize::MAX.rows
        ));
    }

    Ok(size)
}

/// `GET /api/terminal` — one interactive PTY per WebSocket.
pub async fn ws_terminal(
    query: Result<Query<TerminalQuery>, QueryRejection>,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let size = requested_terminal_size(query).map_err(ApiError::bad_request)?;

    Ok(ws
        .max_message_size(crate::terminal::web::MAX_WEB_MESSAGE)
        .max_frame_size(crate::terminal::web::MAX_WEB_MESSAGE)
        .on_upgrade(move |socket| crate::terminal::web::serve_socket(socket, state, size)))
}
