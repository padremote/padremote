//! The settings page's connection, on `/config`.
//!
//! PadRemote's behaviour has always been editable - in a JSON file, with a text
//! editor, on the computer. That is a fine escape hatch and a poor front door:
//! the one device the user is holding is the phone, and the file is on the
//! other machine.
//!
//! So this serves the same config over the same socket the trackpad already
//! uses. It sends three things, because a settings page that shows only the
//! numbers cannot explain itself:
//!
//! - **the file** - what the user has chosen, and what a write here changes
//! - **the effective config** - what the engine is actually running, which
//!   differs wherever the host's own trackpad settings have been mirrored on top
//! - **the host report** - which settings those are, so the page can say "your
//!   computer controls this" rather than silently ignoring an edit
//!
//! Writing goes through the file, deliberately. The file is the source of
//! truth, the app already watches it, and anything that edited only memory
//! would be undone by the next reload - the same trap the phone's own settings
//! fell into.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

use crate::gesture::Config;
use crate::sysprefs::HostTrackpad;

use super::Shared;

/// Which settings the host is deciding.
///
/// Every entry comes from the mirror's own table, so a control the page cannot
/// edit can always name the trackpad setting that took it. This used to carry a
/// second, config-dependent case: three-finger drag overruled both three-finger
/// swipes, and without saying so the page offered an action the next mirror
/// pass wrote `none` straight back over. PadRemote has no three-finger drag any
/// more, so nothing overrules them and the static table is the whole answer.
fn decided_by() -> std::collections::HashMap<String, String> {
    HostTrackpad::controls()
        .into_iter()
        .map(|(field, setting)| (field.to_string(), setting.to_string()))
        .collect()
}

/// What the mirror merely seeds, as opposed to decides.
fn mirror_writes() -> std::collections::HashMap<String, String> {
    HostTrackpad::mirror_actions()
        .into_iter()
        .map(|(field, action)| (field.to_string(), action.to_string()))
        .collect()
}

/// Everything the page needs to draw itself, as one message.
fn state(shared: &Arc<Shared>) -> String {
    let host = HostTrackpad::read();
    let file = shared.file_config();
    let effective = shared.config();
    serde_json::json!({
        "t": "config",
        "computer": shared.host_name,
        // Which system this is, so the settings page can name gestures the way
        // this computer's own trackpad preferences name them. A Mac user is
        // configuring "Mission Control"; the same binding on Windows is Task
        // View, and a page that says the wrong one is describing someone else's
        // machine.
        "os": if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "linux"
        },
        "path": shared.config_path().map(|p| p.display().to_string()),
        // What the user has chosen, and what the page edits.
        "file": file,
        // What the engine is running: the file with the host's own trackpad
        // settings folded on top, wherever `followSystem` lets them win.
        "effective": effective,
        "followSystem": file.follow_system,
        // Which settings the computer is currently deciding, and why - the
        // difference between "this control does nothing" and "your Mac says so".
        "host": host
            .report()
            .iter()
            .map(|r| {
                serde_json::json!({
                    "setting": r.setting,
                    "name": r.name,
                    "value": r.value,
                    "status": r.status.label(),
                    "detail": r.status.detail(),
                })
            })
            .collect::<Vec<_>>(),
        // Which config field each mirrored setting decides, so the page can
        // say *why* a control is not editable. Sent rather than duplicated in
        // the page: the page's own copy drifted the day a row was renamed.
        "decidedBy": decided_by(),
        // What the mirror writes into each binding it owns. A field holding
        // something else is a deliberate choice outside the host's vocabulary,
        // which the mirror now leaves alone - so the page must stop calling it
        // host-decided and stop greying out a control that works.
        "mirrorWrites": mirror_writes(),
        // The values each binding accepts, so the page never offers an action
        // the engine would silently ignore.
        "vocabulary": Config::vocabulary()
            .into_iter()
            .map(|(field, values)| (field.to_string(), values.to_vec()))
            .collect::<std::collections::HashMap<_, _>>(),
    })
    .to_string()
}

/// Serve one settings page.
pub(super) async fn serve(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    peer: SocketAddr,
    shared: Arc<Shared>,
) -> Result<()> {
    tracing::info!("settings page opened: {peer}");
    let (mut tx, mut rx) = ws.split();
    tx.send(Message::Text(state(&shared))).await?;

    // Anything that changes the config - another settings page, an edit to the
    // file, the user flipping a switch in System Settings - lands here too.
    let mut changed = shared.subscribe_config();
    // A settings page can rewrite how this computer behaves, so unpairing must
    // close it along with everything else.
    let mut pairing_rx = shared.subscribe_pairing();

    loop {
        tokio::select! {
            Ok(()) = pairing_rx.changed() => {
                let _ = tx.send(Message::Close(None)).await;
                break;
            }
            Ok(()) = changed.changed() => {
                changed.borrow_and_update();
                tx.send(Message::Text(state(&shared))).await?;
            }
            msg = rx.next() => {
                let Some(msg) = msg else { break };
                match msg? {
                    Message::Text(text) => {
                        if let Some(reply) = handle(&text, &shared) {
                            tx.send(Message::Text(reply)).await?;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        }
    }
    tracing::info!("settings page closed: {peer}");
    Ok(())
}

/// Apply one message from the page. Returns a reply only when something failed:
/// a successful write comes back through the change subscription above, so
/// every open page updates rather than only the one that made the edit.
fn handle(text: &str, shared: &Arc<Shared>) -> Option<String> {
    #[derive(serde::Deserialize)]
    #[serde(tag = "t")]
    enum Msg {
        #[serde(rename = "setConfig")]
        Set { config: Box<Config> },
        #[serde(rename = "reset")]
        Reset,
    }

    let error = |what: &str| Some(serde_json::json!({ "t": "error", "detail": what }).to_string());

    let cfg = match serde_json::from_str::<Msg>(text) {
        Ok(Msg::Set { config }) => *config,
        Ok(Msg::Reset) => Config::default(),
        Err(e) => {
            tracing::debug!("settings page sent something unreadable: {e}");
            return error("that config could not be read");
        }
    };

    match shared.write_config(cfg) {
        Ok(()) => None,
        Err(e) => {
            tracing::error!("could not save the config: {e}");
            error(&format!("could not save: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the page is told is exactly what the mirror does, and nothing else.
    ///
    /// There used to be a third source: three-finger drag overruled both
    /// three-finger swipes, so `decided_by` bolted an extra entry on top of the
    /// table whenever the config had it on. Without that the page offered those
    /// two gestures an action and the next mirror pass wrote `none` straight
    /// back over it, with nothing anywhere explaining why. PadRemote has no
    /// three-finger drag any more, so the tables are the whole answer - and
    /// this test is what keeps a second source from creeping back in.
    #[test]
    fn the_page_is_told_only_what_the_mirror_does() {
        let decided = decided_by();
        assert_eq!(decided.len(), HostTrackpad::controls().len());
        for (field, setting) in HostTrackpad::controls() {
            assert_eq!(decided.get(field).map(String::as_str), Some(setting));
        }

        let writes = mirror_writes();
        assert_eq!(writes.len(), HostTrackpad::mirror_actions().len());
        for (field, action) in HostTrackpad::mirror_actions() {
            assert_eq!(writes.get(field).map(String::as_str), Some(action));
        }
    }
}
