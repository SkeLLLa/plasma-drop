use crate::wm::kwin::client::SharedBridgeState;
use std::time::Instant;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct PlasmaDropDbusObject {
    pub state: SharedBridgeState,
}

#[zbus::interface(name = "ua.SkeLLLa.PlasmaDrop")]
impl PlasmaDropDbusObject {
    #[expect(
        clippy::unused_self,
        reason = "zbus interface methods must take `&self`"
    )]
    fn log(&self, level: &str, msg: &str) {
        match level {
            "ERR" => error!(target: "kwin_script", "{msg}"),
            "WRN" => warn!(target: "kwin_script", "{msg}"),
            _ => info!(target: "kwin_script", "{msg}"),
        }
    }

    async fn get_next_command(&self) -> String {
        self.state.next_command().await
    }

    fn send_response(&self, response_json: &str) {
        if let Err(error) = self.state.resolve_response(response_json) {
            warn!("failed to handle KWin response: {error:#}");
        }
    }

    fn on_press_shortcut(&self, name: &str, modifier: &str, key_char: &str, key_code: &str) {
        let _ = (modifier, key_char, key_code);
        if let Err(error) = self.state.hotkey_tx.send((name.to_owned(), Instant::now())) {
            debug!("hotkey receiver dropped (shutdown): {error}");
        }
    }

    /// `window_id` is the newly active window's id, or empty when no window is active.
    fn on_focus_changed(&self, window_id: &str) {
        let window_id = (!window_id.is_empty()).then(|| window_id.to_owned());
        if self.state.focus_tx.send(window_id).is_err() {
            debug!("focus receiver dropped (shutdown)");
        }
    }
}
