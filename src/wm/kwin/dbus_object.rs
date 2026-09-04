use crate::wm::kwin::client::SharedBridgeState;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct PlasmaDropDbusObject {
    pub state: SharedBridgeState,
}

#[zbus::interface(name = "ua.SkeLLLa.PlasmaDrop")]
impl PlasmaDropDbusObject {
    #[allow(clippy::unused_self)]
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

    async fn send_response(&self, response_json: &str) {
        if let Err(error) = self.state.resolve_response(response_json).await {
            warn!("failed to handle KWin response: {error:#}");
        }
    }

    fn on_press_shortcut(&self, name: &str, modifier: &str, key_char: &str, key_code: &str) {
        let _ = (modifier, key_char, key_code);
        if let Err(error) = self.state.hotkey_tx.send(name.to_string()) {
            debug!("hotkey receiver dropped (shutdown): {error}");
        }
    }

    fn on_focus_changed(&self) {
        if self.state.focus_tx.send(()).is_err() {
            debug!("focus receiver dropped (shutdown)");
        }
    }
}
