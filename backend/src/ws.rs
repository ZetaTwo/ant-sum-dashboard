use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use tokio::sync::watch;
use tower_http::services::ServeDir;

use crate::types::WsMessage;

#[derive(Clone)]
pub struct AppContext {
    pub rx: watch::Receiver<WsMessage>,
}

pub fn router(ctx: AppContext, static_dir: &str) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .fallback_service(ServeDir::new(static_dir))
        .with_state(ctx)
}

async fn ws_handler(ws: WebSocketUpgrade, State(ctx): State<AppContext>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, ctx))
}

async fn handle_socket(mut socket: WebSocket, ctx: AppContext) {
    let mut rx = ctx.rx;

    // Send current state immediately so a freshly-connected client isn't
    // blank until the next device update.
    if !send_current(&mut socket, &mut rx).await {
        return;
    }

    loop {
        if rx.changed().await.is_err() {
            break; // sender dropped
        }
        if !send_current(&mut socket, &mut rx).await {
            break; // client disconnected
        }
    }
}

async fn send_current(socket: &mut WebSocket, rx: &mut watch::Receiver<WsMessage>) -> bool {
    // `borrow_and_update` (not `borrow`) marks this value as seen, so the
    // next `changed()` call only fires for a genuinely new value instead of
    // immediately re-firing for the one we just sent.
    let Ok(json) = serde_json::to_string(&*rx.borrow_and_update()) else {
        return true;
    };
    socket.send(Message::Text(json.into())).await.is_ok()
}
