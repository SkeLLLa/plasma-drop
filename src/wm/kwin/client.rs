use crate::screen::ScreenInfo;
use crate::wm::kwin::proxies::{KGlobalAccelComponentProxy, KGlobalAccelProxy, KWinScriptingProxy};
use crate::wm::kwin::types::{
    CommandEnvelope, CursorPositionResponse, ResponseEnvelope, ScreenEntry, ScreensResponse,
    WindowListResponse, WindowResponse,
};
use crate::wm::{
    Decorations, FrameGeometry, HOTKEY_PREFIX, ManagedWindow, Minimized, Point, WindowManager,
    WindowUpdate,
};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::{Notify, mpsc, oneshot};
use tracing::{debug, error, warn};
use zbus::export::futures_core::Stream;
use zbus::{Connection, connection};

use super::dbus_object::PlasmaDropDbusObject;
use super::systemd;

pub const APP_DBUS_SERVICE: &str = "ua.SkeLLLa.PlasmaDrop";
pub const APP_DBUS_PATH: &str = "/ua/SkeLLLa/PlasmaDrop";

const KWIN_DBUS_SERVICE: &str = "org.kde.KWin";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
/// How long `GetNextCommand` parks before answering with a NOOP. It must stay below the
/// 25 s default timeout of the script's async `callDBus`, or the script's poll errors out.
const LONG_POLL_TIMEOUT: Duration = Duration::from_secs(20);
/// Consecutive command timeouts after which the `KWin` script is considered dead.
const BRIDGE_FAILURE_THRESHOLD: u32 = 3;
/// Responder id 0 is reserved for NOOP frames, which the script never answers.
const NOOP_RESPONDER_ID: u64 = 0;
const NOOP_PAYLOAD: &str = r#"{"type":"NOOP","responderId":0,"params":{}}"#;

/// Locks a std mutex, recovering the data if a previous holder panicked.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug)]
pub(crate) struct BridgeState {
    next_id: AtomicU64,
    queue: Mutex<VecDeque<(u64, String)>>,
    queue_notify: Notify,
    waiters: Mutex<HashMap<u64, oneshot::Sender<ResponseEnvelope>>>,
    consecutive_timeouts: AtomicU32,
    bridge_failed: Notify,
    long_poll_timeout: Duration,
    /// Hotkey presses with the time they reached the daemon.
    pub hotkey_tx: mpsc::UnboundedSender<(String, Instant)>,
    /// Focus changes with the newly active window's id, when the script knew it.
    pub focus_tx: mpsc::UnboundedSender<Option<String>>,
}

pub type SharedBridgeState = Arc<BridgeState>;

impl BridgeState {
    fn new(
        hotkey_tx: mpsc::UnboundedSender<(String, Instant)>,
        focus_tx: mpsc::UnboundedSender<Option<String>>,
        long_poll_timeout: Duration,
    ) -> Self {
        Self {
            next_id: AtomicU64::new(NOOP_RESPONDER_ID + 1),
            queue: Mutex::new(VecDeque::new()),
            queue_notify: Notify::new(),
            waiters: Mutex::new(HashMap::new()),
            consecutive_timeouts: AtomicU32::new(0),
            bridge_failed: Notify::new(),
            long_poll_timeout,
            hotkey_tx,
            focus_tx,
        }
    }

    fn allocate_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn enqueue(&self, id: u64, payload: String) {
        lock(&self.queue).push_back((id, payload));
        self.queue_notify.notify_one();
    }

    /// Drops the waiter and any still-queued payload for `id`, so a command that timed out
    /// is never replayed to the script later.
    fn forget(&self, id: u64) {
        lock(&self.waiters).remove(&id);
        lock(&self.queue).retain(|(queued_id, _)| *queued_id != id);
    }

    /// Counts a command timeout and signals bridge loss once the threshold is reached.
    fn record_timeout(&self) {
        let timeouts = self.consecutive_timeouts.fetch_add(1, Ordering::Relaxed) + 1;
        if timeouts >= BRIDGE_FAILURE_THRESHOLD {
            error!("KWin script did not answer {timeouts} consecutive commands");
            self.bridge_failed.notify_one();
        }
    }

    fn record_success(&self) {
        self.consecutive_timeouts.store(0, Ordering::Relaxed);
    }

    /// Long-polls for the next command, answering with a NOOP once the poll times out.
    pub async fn next_command(&self) -> String {
        let wait = async {
            loop {
                let notified = self.queue_notify.notified();
                let next_item = lock(&self.queue).pop_front();
                if let Some((_, payload)) = next_item {
                    return payload;
                }
                notified.await;
            }
        };
        tokio::time::timeout(self.long_poll_timeout, wait)
            .await
            .unwrap_or_else(|_| NOOP_PAYLOAD.to_owned())
    }

    pub fn resolve_response(&self, response_json: &str) -> Result<()> {
        let response: ResponseEnvelope =
            serde_json::from_str(response_json).context("failed to deserialize response")?;
        let sender = lock(&self.waiters).remove(&response.responder_id);
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
}

impl KWinClient {
    pub async fn connect() -> Result<(
        Self,
        mpsc::UnboundedReceiver<(String, Instant)>,
        mpsc::UnboundedReceiver<Option<String>>,
    )> {
        let (hotkey_tx, hotkey_rx) = mpsc::unbounded_channel();
        let (focus_tx, focus_rx) = mpsc::unbounded_channel();
        let state = Arc::new(BridgeState::new(hotkey_tx, focus_tx, LONG_POLL_TIMEOUT));

        let object = PlasmaDropDbusObject {
            state: Arc::clone(&state),
        };

        let connection = connection::Builder::session()
            .name(APP_DBUS_SERVICE)
            .serve_at(APP_DBUS_PATH, object)
            .build()
            .await
            .with_context(|| {
                format!(
                    "failed to connect to session D-Bus as {APP_DBUS_SERVICE} \
                     (is another plasma-drop instance running?)"
                )
            })?;

        Ok((Self { connection, state }, hotkey_rx, focus_rx))
    }

    /// Resolves once the bridge to the `KWin` script is gone: either `org.kde.KWin` changed
    /// owner (`KWin` restarted or crashed, taking the script with it) or the script stopped
    /// answering commands. The returned error says which one fired.
    pub async fn wait_for_bridge_loss(&self) -> anyhow::Error {
        let mut owner_changes = match self.kwin_owner_changes().await {
            Ok(stream) => stream,
            Err(error) => {
                warn!("{error:#}; only an unresponsive KWin script will be detected");
                self.state.bridge_failed.notified().await;
                return anyhow!("KWin script stopped responding");
            }
        };
        let owner_changed = std::future::poll_fn(|cx| owner_changes.as_mut().poll_next(cx));
        tokio::select! {
            _ = owner_changed => anyhow!("KWin owner changed; the KWin script is gone"),
            () = self.state.bridge_failed.notified() => anyhow!("KWin script stopped responding"),
        }
    }

    async fn kwin_owner_changes(&self) -> Result<Pin<Box<zbus::fdo::NameOwnerChangedStream>>> {
        let proxy = zbus::fdo::DBusProxy::new(&self.connection)
            .await
            .context("failed to create org.freedesktop.DBus proxy")?;
        let stream = proxy
            .receive_name_owner_changed_with_args(&[(0, KWIN_DBUS_SERVICE)])
            .await
            .context("failed to watch the org.kde.KWin name owner")?;
        Ok(Box::pin(stream))
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
            .with_context(|| {
                format!(
                    "failed checking if script '{plugin_name}' is loaded; \
                     KWin scripting interface unavailable; is this Plasma 6 KWin?"
                )
            })?
            && let Err(error) = proxy.unload_script(&plugin_name).await
        {
            warn!("failed to unload previously loaded KWin script '{plugin_name}': {error:#}");
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

    async fn send_command<T>(&self, command_type: &str, params: Value) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let responder_id = self.state.allocate_id();
        let payload = serde_json::to_string(&CommandEnvelope {
            command_type,
            responder_id,
            params,
        })
        .context("failed to serialize command")?;

        let (tx, rx) = oneshot::channel();
        lock(&self.state.waiters).insert(responder_id, tx);
        self.state.enqueue(responder_id, payload);
        debug!("queued command {command_type} ({responder_id})");

        let response = match tokio::time::timeout(COMMAND_TIMEOUT, rx).await {
            Ok(Ok(response)) => {
                self.state.record_success();
                response
            }
            Ok(Err(_)) => {
                self.state.forget(responder_id);
                return Err(anyhow!("response waiter dropped for {command_type}"));
            }
            Err(_) => {
                self.state.forget(responder_id);
                self.state.record_timeout();
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

#[async_trait]
impl WindowManager for KWinClient {
    async fn window_list(&self) -> Result<Vec<ManagedWindow>> {
        let response: WindowListResponse = self.send_command("GET_WINDOW_LIST", json!({})).await?;
        Ok(response.windows)
    }

    async fn window(&self, internal_id: &str) -> Result<Option<ManagedWindow>> {
        let response: WindowResponse = self
            .send_command("GET_WINDOW", json!({ "internalId": internal_id }))
            .await?;
        Ok(response.window)
    }

    async fn active_window(&self) -> Result<Option<ManagedWindow>> {
        let response: WindowResponse = self.send_command("GET_ACTIVE_WINDOW", json!({})).await?;
        Ok(response.window)
    }

    async fn cursor_position(&self) -> Result<Option<Point>> {
        let response: CursorPositionResponse =
            self.send_command("GET_CURSOR_POSITION", json!({})).await?;
        Ok(response.position)
    }

    /// Current enabled screens from `workspace.screens`; empty when `KWin` reports none.
    async fn screens(&self) -> Result<Vec<ScreenInfo>> {
        let response: ScreensResponse = self.send_command("GET_SCREENS", json!({})).await?;
        Ok(screens_from_entries(response.screens))
    }

    async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        self.send_command::<Value>(
            "MOVE_WINDOW",
            json!({ "internalId": internal_id, "x": geometry.x, "y": geometry.y }),
        )
        .await?;
        Ok(())
    }

    async fn resize_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        self.send_command::<Value>(
            "RESIZE_WINDOW",
            json!({ "internalId": internal_id, "width": geometry.width, "height": geometry.height }),
        )
        .await?;
        Ok(())
    }

    async fn set_window_decorations(
        &self,
        internal_id: &str,
        decorations: Decorations,
    ) -> Result<()> {
        let no_border = decorations == Decorations::Hidden;
        self.send_command::<Value>(
            "SET_WINDOW_NO_BORDER",
            json!({ "internalId": internal_id, "noBorder": no_border }),
        )
        .await?;
        Ok(())
    }

    async fn minimize_window(&self, internal_id: &str, minimized: Minimized) -> Result<()> {
        let minimized = minimized == Minimized::Yes;
        self.send_command::<Value>(
            "MINIMIZE_WINDOW",
            json!({ "internalId": internal_id, "minimized": minimized }),
        )
        .await?;
        Ok(())
    }

    async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()> {
        self.send_command::<Value>(
            "BRING_WINDOW_TO_FOREGROUND",
            json!({ "internalId": internal_id }),
        )
        .await?;
        Ok(())
    }

    async fn move_window_to_current_desktop(&self, internal_id: &str) -> Result<()> {
        self.send_command::<Value>(
            "MOVE_WINDOW_TO_CURRENT_DESKTOP",
            json!({ "internalId": internal_id }),
        )
        .await?;
        Ok(())
    }

    async fn update_window(&self, internal_id: &str, update: &WindowUpdate) -> Result<()> {
        self.send_command::<Value>("SET_WINDOW_STATE", window_state_params(internal_id, update))
            .await?;
        Ok(())
    }

    async fn adopt_spawned_process(&self, app_name: &str, pid: u32) -> Result<()> {
        systemd::move_into_scope(&self.connection, app_name, pid).await
    }
}

/// Maps `GET_SCREENS` entries to screens, indexed in list order, dropping outputs without
/// an area.
fn screens_from_entries(entries: Vec<ScreenEntry>) -> Vec<ScreenInfo> {
    entries
        .into_iter()
        .enumerate()
        .filter(|(_, entry)| entry.width > 0 && entry.height > 0)
        .filter_map(|(i, entry)| {
            let index = u32::try_from(i).ok()?;
            let name = if entry.name.is_empty() {
                format!("Screen {i}")
            } else {
                entry.name
            };
            Some(ScreenInfo {
                index,
                name,
                x: entry.x,
                y: entry.y,
                width: entry.width,
                height: entry.height,
            })
        })
        .collect()
}

/// `SET_WINDOW_STATE` params for `update`: position always with geometry, size only when
/// `update.resize`, and opacity clamped to `0.0..=1.0`.
fn window_state_params(internal_id: &str, update: &WindowUpdate) -> Value {
    let mut params = serde_json::Map::new();
    params.insert("internalId".to_owned(), json!(internal_id));
    if let Some(geometry) = &update.geometry {
        params.insert("x".to_owned(), json!(geometry.x));
        params.insert("y".to_owned(), json!(geometry.y));
        if update.resize {
            params.insert("width".to_owned(), json!(geometry.width));
            params.insert("height".to_owned(), json!(geometry.height));
        }
    }
    if let Some(opacity) = update.opacity {
        params.insert("opacity".to_owned(), json!(opacity.clamp(0.0, 1.0)));
    }
    Value::Object(params)
}

#[cfg(test)]
mod helper_tests {
    use super::{screens_from_entries, window_state_params};
    use crate::wm::kwin::types::{ScreenEntry, ScreensResponse};
    use crate::wm::{FrameGeometry, WindowUpdate};
    use serde_json::json;

    fn entry(name: &str, x: i32, width: i32, height: i32) -> ScreenEntry {
        ScreenEntry {
            name: name.into(),
            x,
            y: 0,
            width,
            height,
        }
    }

    #[test]
    fn screens_from_entries_maps_indices_and_names() {
        let screens = screens_from_entries(vec![
            entry("DP-1", 0, 1920, 1080),
            entry("HDMI-A-1", 1920, 2560, 1440),
        ]);
        assert_eq!(screens.len(), 2);
        assert_eq!((screens[0].index, screens[0].name.as_str()), (0, "DP-1"));
        assert_eq!(
            (screens[1].index, screens[1].name.as_str()),
            (1, "HDMI-A-1")
        );
        assert_eq!(
            (screens[1].x, screens[1].width, screens[1].height),
            (1920, 2560, 1440)
        );
    }

    #[test]
    fn screens_from_entries_drops_zero_size_outputs() {
        let screens = screens_from_entries(vec![
            entry("DP-1", 0, 0, 1080),
            entry("DP-2", 0, 1920, 1080),
        ]);
        assert_eq!(screens.len(), 1);
        assert_eq!(screens[0].name, "DP-2");
        assert_eq!(screens[0].index, 1);
    }

    #[test]
    fn screens_from_entries_names_unnamed_outputs_by_index() {
        let screens = screens_from_entries(vec![
            entry("DP-1", 0, 1920, 1080),
            entry("", 1920, 1920, 1080),
        ]);
        assert_eq!(screens[1].name, "Screen 1");
    }

    #[test]
    fn screens_response_deserializes_negative_origins() {
        let response: ScreensResponse = serde_json::from_str(
            r#"{"screens":[{"name":"DP-2","x":-1920,"y":-1080,"width":1920,"height":1080}]}"#,
        )
        .unwrap();
        let screens = screens_from_entries(response.screens);
        assert_eq!(screens.len(), 1);
        assert_eq!(
            (screens[0].name.as_str(), screens[0].x, screens[0].y),
            ("DP-2", -1920, -1080)
        );
        assert_eq!((screens[0].width, screens[0].height), (1920, 1080));
    }

    #[test]
    fn window_state_params_omits_size_without_resize() {
        let update = WindowUpdate {
            geometry: Some(FrameGeometry {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            }),
            resize: false,
            opacity: None,
        };
        assert_eq!(
            window_state_params("{a}", &update),
            json!({ "internalId": "{a}", "x": 1, "y": 2 })
        );
    }

    #[test]
    fn window_state_params_includes_size_and_clamps_opacity() {
        let update = WindowUpdate {
            geometry: Some(FrameGeometry {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            }),
            resize: true,
            opacity: Some(1.5),
        };
        assert_eq!(
            window_state_params("{a}", &update),
            json!({ "internalId": "{a}", "x": 1, "y": 2, "width": 3, "height": 4, "opacity": 1.0 })
        );
        let fade_only = WindowUpdate {
            geometry: None,
            resize: true,
            opacity: Some(-0.5),
        };
        assert_eq!(
            window_state_params("{a}", &fade_only),
            json!({ "internalId": "{a}", "opacity": 0.0 })
        );
    }
}

#[cfg(test)]
mod bridge_state_tests {
    use super::{BRIDGE_FAILURE_THRESHOLD, BridgeState, NOOP_PAYLOAD, lock};
    use crate::wm::kwin::types::ResponseEnvelope;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};

    fn test_state_with_poll(long_poll_timeout: Duration) -> BridgeState {
        let (hotkey_tx, _hotkey_rx) = mpsc::unbounded_channel();
        let (focus_tx, _focus_rx) = mpsc::unbounded_channel();
        BridgeState::new(hotkey_tx, focus_tx, long_poll_timeout)
    }

    fn test_state() -> BridgeState {
        test_state_with_poll(Duration::from_secs(60))
    }

    #[tokio::test]
    async fn next_command_returns_enqueued_payloads_in_order() {
        let state = test_state();
        state.enqueue(1, "first".to_string());
        state.enqueue(2, "second".to_string());

        assert_eq!(state.next_command().await, "first");
        assert_eq!(state.next_command().await, "second");
    }

    #[tokio::test]
    async fn next_command_waits_until_something_is_enqueued() {
        let state = Arc::new(test_state());
        let waiter = tokio::spawn({
            let state = Arc::clone(&state);
            async move { state.next_command().await }
        });

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !waiter.is_finished(),
            "next_command should still be waiting with an empty queue"
        );

        state.enqueue(1, "late".to_string());
        let received = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("next_command should resolve once a command is enqueued")
            .unwrap();
        assert_eq!(received, "late");
    }

    #[tokio::test]
    async fn next_command_returns_noop_after_long_poll_timeout() {
        let state = test_state_with_poll(Duration::from_millis(10));
        let received = tokio::time::timeout(Duration::from_secs(1), state.next_command())
            .await
            .expect("long poll should end with a NOOP");
        assert_eq!(received, NOOP_PAYLOAD);
        let noop: serde_json::Value = serde_json::from_str(&received).unwrap();
        assert_eq!(noop["type"], "NOOP");
        assert_eq!(noop["responderId"], 0);
    }

    #[tokio::test]
    async fn timed_out_command_is_purged_from_queue() {
        let state = test_state();
        let (tx, _rx) = oneshot::channel();
        lock(&state.waiters).insert(7, tx);
        state.enqueue(7, "stale".to_string());
        state.enqueue(8, "fresh".to_string());

        state.forget(7);

        assert!(lock(&state.waiters).is_empty());
        assert_eq!(state.next_command().await, "fresh");
        assert!(lock(&state.queue).is_empty());
    }

    #[tokio::test]
    async fn threshold_timeouts_signal_bridge_failure() {
        let state = test_state();
        for _ in 0..BRIDGE_FAILURE_THRESHOLD {
            state.record_timeout();
        }
        tokio::time::timeout(Duration::from_secs(1), state.bridge_failed.notified())
            .await
            .expect("bridge failure should be signalled after repeated timeouts");
    }

    #[test]
    fn resolve_response_delivers_to_matching_waiter_and_removes_it() {
        let state = test_state();
        let (tx, mut rx) = oneshot::channel();
        lock(&state.waiters).insert(42, tx);

        state
            .resolve_response(r#"{"cmdType":"GET_WINDOW","responderId":42,"params":{"ok":true}}"#)
            .unwrap();

        let response = rx
            .try_recv()
            .expect("the waiter should receive the response");
        assert_eq!(response.responder_id, 42);
        assert_eq!(response.params["ok"], true);
        assert!(lock(&state.waiters).is_empty());
    }

    #[test]
    fn resolve_response_for_unknown_responder_is_not_an_error() {
        let state = test_state();
        let (tx, _rx) = oneshot::channel();
        lock(&state.waiters).insert(1, tx);

        state
            .resolve_response(r#"{"cmdType":"GET_WINDOW","responderId":99,"params":{}}"#)
            .unwrap();

        assert!(lock(&state.waiters).contains_key(&1));
    }

    #[test]
    fn resolve_response_rejects_malformed_json() {
        let state = test_state();
        assert!(state.resolve_response("{not json").is_err());
    }

    #[test]
    fn response_envelope_deserializes_expected_wire_format() {
        // Guards the field renames (`cmdType`/`responderId`) that
        // `resolve_response` depends on to parse the KWin script's replies.
        let value: ResponseEnvelope = serde_json::from_str(
            r#"{"cmdType":"GET_WINDOW_LIST","responderId":17,"params":{},"exception_message":"boom"}"#,
        )
        .unwrap();
        assert_eq!(value.cmd_type, "GET_WINDOW_LIST");
        assert_eq!(value.responder_id, 17);
        assert_eq!(value.exception_message.as_deref(), Some("boom"));
    }
}
