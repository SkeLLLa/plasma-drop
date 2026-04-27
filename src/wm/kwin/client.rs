use crate::wm::kwin::proxies::{
    KGlobalAccelComponentProxy, KGlobalAccelProxy, KWinProxy, KWinScriptingProxy,
};
use crate::wm::kwin::types::{
    CommandEnvelope, CursorPositionResponse, ResponseEnvelope, SupportInformationResponse,
    WindowListResponse, WindowResponse,
};
use crate::wm::{FrameGeometry, HOTKEY_PREFIX, ManagedWindow, Point, WindowManager};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use tracing::{debug, warn};
use uuid::Uuid;
use zbus::{Connection, connection};

use super::dbus_object::PlasmaDropDbusObject;

pub const APP_DBUS_SERVICE: &str = "ua.SkeLLLa.PlasmaDrop";
pub const APP_DBUS_PATH: &str = "/ua/SkeLLLa/PlasmaDrop";

const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(crate) struct BridgeState {
    queue: Mutex<VecDeque<String>>,
    queue_notify: Notify,
    waiters: Mutex<HashMap<Uuid, oneshot::Sender<ResponseEnvelope>>>,
    last_enqueued: Mutex<Instant>,
    pub hotkey_tx: mpsc::UnboundedSender<String>,
}

pub type SharedBridgeState = Arc<BridgeState>;

impl BridgeState {
    async fn enqueue(&self, payload: String) {
        self.queue.lock().await.push_back(payload);
        *self.last_enqueued.lock().await = Instant::now();
        self.queue_notify.notify_one();
    }

    pub async fn next_command(&self) -> String {
        loop {
            let notified = self.queue_notify.notified();
            let next_item = self.queue.lock().await.pop_front();
            if let Some(item) = next_item {
                return item;
            }
            notified.await;
        }
    }

    pub async fn resolve_response(&self, response_json: &str) -> Result<()> {
        let response: ResponseEnvelope =
            serde_json::from_str(response_json).context("failed to deserialize response")?;
        let sender = self.waiters.lock().await.remove(&response.responder_id);
        if let Some(sender) = sender {
            if sender.send(response).is_err() {
                debug!("response receiver dropped (likely timed out)");
            }
        } else {
            warn!(
                "received response for unknown responder {}",
                response.responder_id
            );
        }
        Ok(())
    }
}

pub struct KWinClient {
    connection: Connection,
    state: SharedBridgeState,
    keepalive_task: tokio::task::JoinHandle<()>,
}

impl KWinClient {
    pub async fn connect() -> Result<(Self, mpsc::UnboundedReceiver<String>)> {
        let (hotkey_tx, hotkey_rx) = mpsc::unbounded_channel();
        let state = Arc::new(BridgeState {
            queue: Mutex::new(VecDeque::new()),
            queue_notify: Notify::new(),
            waiters: Mutex::new(HashMap::new()),
            last_enqueued: Mutex::new(Instant::now()),
            hotkey_tx,
        });

        let object = PlasmaDropDbusObject {
            state: state.clone(),
        };

        let connection = connection::Builder::session()
            .context("failed to connect to session D-Bus")?
            .name(APP_DBUS_SERVICE)
            .context("failed to acquire single-instance D-Bus name")?
            .serve_at(APP_DBUS_PATH, object)
            .context("failed to register D-Bus object")?
            .build()
            .await
            .context("failed to build D-Bus connection")?;

        let keepalive_state = state.clone();
        let keepalive_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(KEEPALIVE_INTERVAL).await;
                let last = *keepalive_state.last_enqueued.lock().await;
                if last.elapsed() >= KEEPALIVE_INTERVAL {
                    let payload = match serde_json::to_string(&CommandEnvelope {
                        command_type: "NOOP".into(),
                        responder_id: Uuid::new_v4(),
                        params: json!({}),
                    }) {
                        Ok(payload) => payload,
                        Err(error) => {
                            warn!("failed to serialize NOOP keepalive: {error:#}");
                            continue;
                        }
                    };
                    keepalive_state.enqueue(payload).await;
                }
            }
        });

        Ok((
            Self {
                connection,
                state,
                keepalive_task,
            },
            hotkey_rx,
        ))
    }

    pub async fn ensure_compatibility(&self) -> Result<()> {
        KWinProxy::new(&self.connection)
            .await
            .context("failed to create org.kde.KWin proxy")?
            .support_information()
            .await
            .context("failed to query KWin support information")?;
        Ok(())
    }

    pub async fn load_script(&self, path: &Path) -> Result<String> {
        let proxy = KWinScriptingProxy::new(&self.connection)
            .await
            .context("failed to create KWin scripting proxy")?;
        let plugin_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("script path has no file name")?
            .to_string();

        if proxy
            .is_script_loaded(&plugin_name)
            .await
            .with_context(|| format!("failed checking if script '{plugin_name}' is loaded"))?
        {
            let _ = proxy.unload_script(&plugin_name).await;
        }

        proxy
            .load_script(path.to_string_lossy().as_ref(), &plugin_name)
            .await
            .with_context(|| format!("failed to load KWin script '{}'", path.display()))?;
        proxy.start().await.context("failed to start KWin script")?;

        Ok(plugin_name)
    }

    pub async fn unload_script(&self, plugin_name: &str) -> Result<()> {
        let proxy = KWinScriptingProxy::new(&self.connection)
            .await
            .context("failed to create KWin scripting proxy")?;
        let _ = proxy
            .unload_script(plugin_name)
            .await
            .with_context(|| format!("failed to unload KWin script '{plugin_name}'"))?;
        Ok(())
    }

    pub async fn cleanup_shortcuts(&self) -> Result<()> {
        let accel = KGlobalAccelProxy::new(&self.connection)
            .await
            .context("failed to create KGlobalAccel proxy")?;
        let component = KGlobalAccelComponentProxy::new(&self.connection)
            .await
            .context("failed to create KWin shortcut component proxy")?;

        let names = component
            .shortcut_names()
            .await
            .context("failed to fetch existing KWin shortcuts")?;
        for name in names {
            if name.starts_with(HOTKEY_PREFIX) {
                match accel.unregister("kwin", &name).await {
                    Ok(true) => {}
                    Ok(false) => warn!("shortcut '{name}' was not unregistered"),
                    Err(error) => warn!("failed to unregister shortcut '{name}': {error:#}"),
                }
            }
        }

        if let Err(error) = component.clean_up().await {
            warn!("failed to clean up KWin shortcut component: {error:#}");
        }
        Ok(())
    }

    pub async fn register_hotkey(&self, id: &str, title: &str, sequence: &str) -> Result<()> {
        let params = json!({
            "name": id,
            "title": title,
            "sequence": sequence
        });
        self.send_command::<Value>("REGISTER_HOT_KEY", params)
            .await?;
        Ok(())
    }

    pub async fn get_window_list(&self) -> Result<Vec<ManagedWindow>> {
        let response: WindowListResponse = self.send_command("GET_WINDOW_LIST", json!({})).await?;
        Ok(response.windows)
    }

    pub async fn get_window(&self, internal_id: &str) -> Result<Option<ManagedWindow>> {
        let response: WindowResponse = self
            .send_command("GET_WINDOW", json!({ "internalId": internal_id }))
            .await?;
        Ok(response.window)
    }

    pub async fn get_active_window(&self) -> Result<Option<ManagedWindow>> {
        let response: WindowResponse = self.send_command("GET_ACTIVE_WINDOW", json!({})).await?;
        Ok(response.window)
    }

    pub async fn get_cursor_position(&self) -> Result<Option<Point>> {
        let response: CursorPositionResponse =
            self.send_command("GET_CURSOR_POSITION", json!({})).await?;
        Ok(response.position)
    }

    pub async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        self.send_command::<Value>(
            "MOVE_WINDOW",
            json!({ "internalId": internal_id, "x": geometry.x, "y": geometry.y }),
        )
        .await?;
        Ok(())
    }

    pub async fn resize_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        self.send_command::<Value>(
            "RESIZE_WINDOW",
            json!({ "internalId": internal_id, "width": geometry.width, "height": geometry.height }),
        )
        .await?;
        Ok(())
    }

    pub async fn set_window_opacity(&self, internal_id: &str, opacity: f64) -> Result<()> {
        self.send_command::<Value>(
            "SET_WINDOW_OPACITY",
            json!({ "internalId": internal_id, "opacity": opacity.clamp(0.0, 1.0) }),
        )
        .await?;
        Ok(())
    }

    pub async fn set_window_no_border(&self, internal_id: &str, no_border: bool) -> Result<()> {
        self.send_command::<Value>(
            "SET_WINDOW_NO_BORDER",
            json!({ "internalId": internal_id, "noBorder": no_border }),
        )
        .await?;
        Ok(())
    }

    pub async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()> {
        self.send_command::<Value>(
            "BRING_WINDOW_TO_FOREGROUND",
            json!({ "internalId": internal_id }),
        )
        .await?;
        Ok(())
    }

    pub async fn get_support_information_text(&self) -> Result<String> {
        let response: SupportInformationResponse = self
            .send_command("GET_SUPPORT_INFORMATION", json!({}))
            .await?;
        Ok(response.text)
    }

    async fn send_command<T>(&self, command_type: &str, params: Value) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let responder_id = Uuid::new_v4();
        let payload = serde_json::to_string(&CommandEnvelope {
            command_type: command_type.to_string(),
            responder_id,
            params,
        })
        .context("failed to serialize command")?;

        let (tx, rx) = oneshot::channel();
        self.state.waiters.lock().await.insert(responder_id, tx);
        self.state.enqueue(payload).await;
        debug!("queued command {command_type} ({responder_id})");

        let response = match tokio::time::timeout(COMMAND_TIMEOUT, rx).await {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => {
                self.state.waiters.lock().await.remove(&responder_id);
                return Err(anyhow!("response waiter dropped for {command_type}"));
            }
            Err(_) => {
                self.state.waiters.lock().await.remove(&responder_id);
                bail!("timed out waiting for KWin response to {command_type}");
            }
        };

        if let Some(error) = response.exception_message {
            bail!("KWin command {} failed: {}", response.cmd_type, error);
        }

        serde_json::from_value(response.params)
            .with_context(|| format!("failed to parse response params for {command_type}"))
    }
}

impl Drop for KWinClient {
    fn drop(&mut self) {
        self.keepalive_task.abort();
    }
}

#[async_trait]
impl WindowManager for KWinClient {
    async fn get_window_list(&self) -> Result<Vec<ManagedWindow>> {
        Self::get_window_list(self).await
    }

    async fn get_window(&self, internal_id: &str) -> Result<Option<ManagedWindow>> {
        Self::get_window(self, internal_id).await
    }

    async fn get_active_window(&self) -> Result<Option<ManagedWindow>> {
        Self::get_active_window(self).await
    }

    async fn get_cursor_position(&self) -> Result<Option<Point>> {
        Self::get_cursor_position(self).await
    }

    async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        Self::move_window(self, internal_id, geometry).await
    }

    async fn resize_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        Self::resize_window(self, internal_id, geometry).await
    }

    async fn set_window_opacity(&self, internal_id: &str, opacity: f64) -> Result<()> {
        Self::set_window_opacity(self, internal_id, opacity).await
    }

    async fn set_window_no_border(&self, internal_id: &str, no_border: bool) -> Result<()> {
        Self::set_window_no_border(self, internal_id, no_border).await
    }

    async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()> {
        Self::bring_window_to_foreground(self, internal_id).await
    }
}
