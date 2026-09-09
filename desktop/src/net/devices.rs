//! The devices channel, on `/devices`: who is paired, and how to un-pair them.
//!
//! Managing paired devices used to live in the menu bar - a submenu of names to
//! forget, and an "Unpair all" beside it. That is the wrong place for it twice
//! over: a menu that has to be held open cannot show a list that changes, and
//! the person deciding whether the spare iPad should still be able to move this
//! cursor wants to *read* the list, not hover over it. So the list moved to the
//! page the user already opens to pair a phone, and this is what feeds it.
//!
//! Two rules keep the channel honest:
//!
//! - **Reading is for anyone who has answered the challenge.** The connect page
//!   is served only to this computer, but a phone could open this socket too,
//!   and knowing which devices are paired is no more than it already knows.
//! - **Un-pairing is for this computer alone.** Revocation is checked against
//!   the TCP peer being loopback, not against anything the caller says about
//!   itself, so a phone cannot revoke the phone next to it.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

use super::Shared;

/// One JSON message with everything the page draws.
///
/// Paired devices and connected ones are two different lists - a phone in a bag
/// downstairs is paired and not connected, and the replay tool is connected and
/// not paired - so both are merged here rather than in the page, which would
/// otherwise have to know why an id can be missing.
fn state(shared: &Arc<Shared>, may_manage: bool) -> String {
    let view = shared.control_view();
    let mut rows: Vec<serde_json::Value> = shared
        .paired
        .list()
        .into_iter()
        .map(|(id, known)| {
            let live = view
                .list
                .iter()
                .find(|d| d.device_id.as_deref() == Some(id.as_str()));
            serde_json::json!({
                "id": id,
                "mac": known.mac,
                // The name it is calling itself right now beats the one it
                // enrolled under: a phone that has been renamed should not be
                // listed for ever as whatever it was called on day one.
                "name": live.map_or_else(|| known.name.clone(), |d| d.label.clone()),
                "connected": live.is_some(),
                "driving": live.is_some_and(|d| d.driving),
                "since": known.first_seen,
            })
        })
        .collect();
    // Several authenticated browsers can belong to the same LAN device.
    // Prefer its live row and retain every credential for group revocation.
    let mut grouped: Vec<serde_json::Value> = Vec::new();
    for row in rows {
        let existing = row["mac"].as_str().and_then(|mac| {
            grouped
                .iter_mut()
                .find(|other| other["mac"].as_str() == Some(mac))
        });
        if let Some(existing) = existing {
            let since = existing["since"]
                .as_u64()
                .unwrap_or(0)
                .min(row["since"].as_u64().unwrap_or(0));
            if row["connected"] == true {
                *existing = row;
            }
            existing["since"] = since.into();
        } else {
            grouped.push(row);
        }
    }
    rows = grouped;
    // Connected without a credential: the replay tool and the tests, which
    // authenticate with the QR secret and never enrol. Listed so the rows add
    // up to the count above them, and offered no Forget, because there is no
    // credential to revoke.
    rows.extend(view.list.iter().filter(|d| d.device_id.is_none()).map(|d| {
        serde_json::json!({
            "id": serde_json::Value::Null,
            "name": d.label,
            "connected": true,
            "driving": d.driving,
            "since": serde_json::Value::Null,
        })
    }));
    // Connected first, then alphabetical: the row the user is looking for is
    // almost always the phone in their hand, and a list that reorders itself
    // as devices come and go is a list you cannot click.
    rows.sort_by(|a, b| {
        let key = |v: &serde_json::Value| {
            (
                !v["connected"].as_bool().unwrap_or(false),
                v["name"].as_str().unwrap_or_default().to_lowercase(),
            )
        };
        key(a).cmp(&key(b))
    });
    serde_json::json!({
        "t": "devices",
        "connected": rows.iter().filter(|row| row["connected"] == true).count(),
        "manage": may_manage,
        // The address the QR on the open page was drawn around. When the router
        // hands out a different one, every code and printed URL from before
        // points at nothing, so the page reloads itself rather than showing a
        // phone a code that cannot work.
        "host": shared.lan_ip(),
        "devices": rows,
    })
    .to_string()
}

pub(super) async fn serve(
    ws: WebSocketStream<TcpStream>,
    peer: SocketAddr,
    shared: Arc<Shared>,
) -> Result<()> {
    // Only this computer may revoke a pairing. Decided from the TCP peer, once,
    // here - never from anything the page says about itself.
    let may_manage = peer.ip().is_loopback();
    let (mut tx, mut rx) = ws.split();

    let mut revoked = shared.subscribe_pairing();
    let mut moved = shared.subscribe_lan();
    let mut changed = shared.subscribe_control();
    changed.mark_changed();
    // Sent only when it differs, so a phone twitching under a finger does not
    // redraw a list of names sixty times a second.
    let mut previous: Option<String> = None;
    macro_rules! push {
        () => {{
            let now = state(&shared, may_manage);
            if previous.as_deref() != Some(now.as_str()) {
                tx.send(Message::Text(now.clone())).await?;
                previous = Some(now);
            }
        }};
    }

    loop {
        tokio::select! {
            result = changed.changed() => {
                if result.is_err() { break; }
                push!();
            }
            result = moved.changed() => {
                if result.is_err() { break; }
                push!();
            }
            _ = revoked.changed() => {
                // The secret this page holds has just been replaced - by its
                // own Forget all, or by another page's. Nothing it knows opens
                // a socket any more, so it is closed and reloads itself.
                let _ = tx.send(Message::Close(None)).await;
                break;
            }
            message = rx.next() => {
                match message {
                    None | Some(Ok(Message::Close(_))) => break,
                    Some(Err(error)) => return Err(error.into()),
                    Some(Ok(Message::Text(text))) => {
                        if let Some(reply) = handle(&text, &shared, may_manage) {
                            tx.send(Message::Text(reply)).await?;
                        }
                        // Forgetting a device that was not connected changes
                        // nothing the control view watches, so the answer is
                        // pushed here rather than waited for.
                        push!();
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

/// Apply one message. Returns a reply only when something was refused - a
/// success comes back as the new list, which every open page gets.
fn handle(text: &str, shared: &Arc<Shared>, may_manage: bool) -> Option<String> {
    #[derive(serde::Deserialize)]
    #[serde(tag = "t")]
    enum Msg {
        /// Revoke one device: its key stops working, here and after a restart.
        #[serde(rename = "forget")]
        Forget { id: String },
        /// Revoke every device and rotate the QR secret with them.
        #[serde(rename = "forgetAll")]
        ForgetAll,
    }

    let error = |what: &str| Some(serde_json::json!({ "t": "error", "detail": what }).to_string());

    let msg = match serde_json::from_str::<Msg>(text) {
        Ok(msg) => msg,
        Err(e) => {
            tracing::debug!("the devices page sent something unreadable: {e}");
            return error("that request could not be read");
        }
    };
    if !may_manage {
        tracing::warn!("refusing to change pairing for a caller that is not this computer");
        return error("only this computer can change pairing");
    }
    match msg {
        Msg::Forget { id } => {
            if shared.forget_device(&id) {
                tracing::info!("forgot a device");
            }
            None
        }
        Msg::ForgetAll => {
            shared.unpair_all();
            tracing::info!("unpaired every device");
            None
        }
    }
}
