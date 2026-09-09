//! The read-only view on `/observe`.
//!
//! Whatever the phones are sending, mirrored to anyone watching - the debug
//! page, usually running on the computer itself. An observer never claims the
//! cursor and never appears in the device list, which is what makes it safe to
//! leave open while somebody drives.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

use super::Shared;

pub(super) async fn observe(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    peer: SocketAddr,
    shared: Arc<Shared>,
) -> Result<()> {
    tracing::info!("observer attached: {peer}");
    let mut rx = shared.telemetry.subscribe();
    let mut pairing_rx = shared.subscribe_pairing();
    let (mut tx, _read) = ws.split();

    // Say what we are mirroring straight away, so a watcher that joins mid
    // session is not staring at a blank page.
    let hello = serde_json::json!({
        "t": "hello",
        "computer": shared.host_name,
        "settings": crate::sysprefs::HostTrackpad::read()
            .report()
            .iter()
            .map(|r| serde_json::json!({
                "setting": r.setting,
                "value": r.value,
                "status": r.status.label(),
                "detail": r.status.detail(),
            }))
            .collect::<Vec<_>>(),
    });
    tx.send(Message::Text(hello.to_string())).await?;

    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Ok(line) => tx.send(Message::Text(line)).await?,
                // Falling behind is fine; skipping to the present is the right
                // behaviour for a live view.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            },
            // Unpairing revokes the debug view too - it is a live mirror of
            // every touch, so leaving it open would be the leak.
            Ok(()) = pairing_rx.changed() => {
                let _ = tx.send(Message::Close(None)).await;
                break;
            }
        }
    }
    tracing::info!("observer detached: {peer}");
    Ok(())
}
