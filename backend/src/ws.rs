use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum_embed::ServeEmbed;
use rust_embed::RustEmbed;
use tokio::sync::watch;

use crate::types::WsMessage;

/// The built frontend (`frontend/dist`). In debug builds `rust-embed` reads
/// these from disk on every request instead of embedding them, so
/// `cargo run`/`dev-hardware` see frontend changes without a backend
/// rebuild; release builds embed the files into the binary, so a shipped
/// `.exe` needs no separate `frontend/dist` alongside it.
#[derive(RustEmbed, Clone)]
#[folder = "../frontend/dist"]
struct Frontend;

#[derive(Clone)]
pub struct AppContext {
    pub rx: watch::Receiver<WsMessage>,
}

pub fn router(ctx: AppContext) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .fallback_service(ServeEmbed::<Frontend>::new())
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
