mod usb;

use tokio::sync::mpsc;

use crate::types::DeviceUpdate;

/// Mirrors `simulator::run`'s signature so `main.rs` can pick either source
/// without the rest of the pipeline (state/ws) knowing the difference.
pub async fn run(tx: mpsc::Sender<DeviceUpdate>) -> anyhow::Result<()> {
    usb::run(tx).await
}
