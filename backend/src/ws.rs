use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use tokio::sync::watch;

use crate::types::WsMessage;

/// Release builds embed `frontend/dist` into the binary at compile time, so
/// a shipped `.exe` needs no separate `frontend/dist` alongside it. Debug
/// builds instead serve straight from disk via `ServeDir`, so `cargo run`/
/// `dev-hardware` see frontend changes without a backend rebuild. The two
/// are behind a `cfg(debug_assertions)` split (rather than relying on
/// `rust-embed`'s own release/debug branching within one `RustEmbed` impl)
/// so that in debug/test builds `#[derive(RustEmbed)]` is never compiled at
/// all, and `frontend/dist` doesn't need to exist for `cargo test`/`cargo
/// build` to succeed on a fresh clone that hasn't built the frontend yet.
#[cfg(not(debug_assertions))]
mod frontend {
    use axum_embed::ServeEmbed;
    use rust_embed::RustEmbed;

    #[derive(RustEmbed, Clone)]
    #[folder = "../frontend/dist"]
    pub(super) struct Frontend;

    pub fn service() -> ServeEmbed<Frontend> {
        ServeEmbed::<Frontend>::new()
    }
}

#[cfg(debug_assertions)]
mod frontend {
    use tower_http::services::ServeDir;

    pub fn service() -> ServeDir {
        ServeDir::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../frontend/dist"))
    }
}

#[derive(Clone)]
pub struct AppContext {
    pub rx: watch::Receiver<WsMessage>,
}

pub fn router(ctx: AppContext) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .fallback_service(frontend::service())
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
