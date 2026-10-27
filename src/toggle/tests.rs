use super::{HOTKEY_DEBOUNCE_WINDOW, SPAWN_POLL_TIMEOUT, ToggleService, should_ignore_shortcut};
use crate::app_registry::{AppRegistry, ManagedApp};
use crate::config::{
    AnimationConfig, AnimationDirection, AnimationEasing, AnimationStyle, AppConfig, AttachMode,
    HideBehavior, PlacementConfig, PlacementMetric, PlacementPosition,
};
use crate::hotkey::Hotkey;
use crate::screen::ScreenInfo;
use crate::wm::{
    Decorations, FrameGeometry, ManagedWindow, Minimized, Point, WindowManager, WindowUpdate,
};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[test]
fn ignores_repeated_shortcut_within_debounce_window() {
    let now = Instant::now();
    let mut recent_hotkeys = HashMap::new();

    assert!(!should_ignore_shortcut(&mut recent_hotkeys, "test", now));
    assert!(should_ignore_shortcut(
        &mut recent_hotkeys,
        "test",
        now + Duration::from_millis(50)
    ));
    assert!(!should_ignore_shortcut(
        &mut recent_hotkeys,
        "test",
        now + HOTKEY_DEBOUNCE_WINDOW + Duration::from_millis(20)
    ));
}

#[test]
fn debounce_uses_press_time_in_any_order() {
    let t0 = Instant::now();
    let mut recent_hotkeys = HashMap::new();

    assert!(!should_ignore_shortcut(
        &mut recent_hotkeys,
        "test",
        t0 + Duration::from_millis(200)
    ));
    // An earlier press handled later is still within the window and does not panic.
    assert!(should_ignore_shortcut(&mut recent_hotkeys, "test", t0));
}

#[tokio::test]
async fn fresh_hotkey_toggles_the_app() {
    let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let shortcut_id = managed.shortcut_id.clone();
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service
        .handle_shortcut(&shortcut_id, Instant::now())
        .await
        .unwrap();

    assert_eq!(
        registry.lock().await.currently_visible_name(),
        Some("dolphin")
    );
}

#[derive(Default)]
struct MockKWin {
    calls: Mutex<Vec<String>>,
    window: Mutex<Option<ManagedWindow>>,
    active_window: Mutex<Option<ManagedWindow>>,
    cursor_position: Mutex<Option<Point>>,
    windows: Mutex<Vec<ManagedWindow>>,
    /// Screens reported by `screens`; empty means "use the cache".
    screens: Mutex<Vec<ScreenInfo>>,
    /// Number of `screens` calls (screen refreshes).
    info_requests: AtomicUsize,
    /// Number of `window` calls.
    window_requests: AtomicUsize,
    /// Window ids for which `window` and `move_window` fail.
    fail_window_ids: Vec<String>,
    /// Makes every `move_window` call fail.
    fail_moves: bool,
    /// `window_list` calls made so far.
    window_list_calls: AtomicUsize,
    /// `window_list` fails from this (0-based) call on.
    fail_window_list_from: Option<usize>,
    /// `window_list` reports no windows for this many calls, e.g. until a spawned app opens.
    windows_hidden_for: usize,
    /// Makes `adopt_spawned_process` fail.
    fail_adopt: bool,
}

impl MockKWin {
    fn fails_for(&self, internal_id: &str) -> bool {
        self.fail_window_ids.iter().any(|id| id == internal_id)
    }
}

#[async_trait]
impl WindowManager for MockKWin {
    async fn window_list(&self) -> Result<Vec<ManagedWindow>> {
        let call = self.window_list_calls.fetch_add(1, Ordering::Relaxed);
        if self.fail_window_list_from.is_some_and(|from| call >= from) {
            bail!("window list failed");
        }
        if call < self.windows_hidden_for {
            return Ok(Vec::new());
        }
        Ok(self.windows.lock().await.clone())
    }

    async fn window(&self, internal_id: &str) -> Result<Option<ManagedWindow>> {
        self.window_requests.fetch_add(1, Ordering::Relaxed);
        if self.fails_for(internal_id) {
            bail!("window failed for {internal_id}");
        }
        Ok(self.window.lock().await.clone())
    }

    async fn active_window(&self) -> Result<Option<ManagedWindow>> {
        Ok(self.active_window.lock().await.clone())
    }

    async fn cursor_position(&self) -> Result<Option<Point>> {
        Ok(*self.cursor_position.lock().await)
    }

    async fn screens(&self) -> Result<Vec<ScreenInfo>> {
        self.info_requests.fetch_add(1, Ordering::Relaxed);
        Ok(self.screens.lock().await.clone())
    }

    async fn update_window(&self, internal_id: &str, update: &WindowUpdate) -> Result<()> {
        if let Some(geometry) = &update.geometry {
            self.move_window(internal_id, geometry).await?;
            if update.resize {
                self.resize_window(internal_id, geometry).await?;
            }
        }
        if let Some(opacity) = update.opacity {
            self.calls
                .lock()
                .await
                .push(format!("opacity:{internal_id}:{opacity:.3}"));
        }
        Ok(())
    }

    async fn adopt_spawned_process(&self, app_name: &str, _pid: u32) -> Result<()> {
        self.calls.lock().await.push(format!("adopt:{app_name}"));
        if self.fail_adopt {
            bail!("adopt failed for {app_name}");
        }
        Ok(())
    }

    async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        if self.fail_moves || self.fails_for(internal_id) {
            bail!("move failed for {internal_id}");
        }
        self.calls.lock().await.push(format!(
            "move:{internal_id}:{}:{}:{}:{}",
            geometry.x, geometry.y, geometry.width, geometry.height
        ));
        Ok(())
    }

    async fn resize_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        self.calls.lock().await.push(format!(
            "resize:{internal_id}:{}:{}:{}:{}",
            geometry.x, geometry.y, geometry.width, geometry.height
        ));
        Ok(())
    }

    async fn set_window_decorations(
        &self,
        internal_id: &str,
        decorations: Decorations,
    ) -> Result<()> {
        let no_border = decorations == Decorations::Hidden;
        self.calls
            .lock()
            .await
            .push(format!("no_border:{internal_id}:{no_border}"));
        Ok(())
    }

    async fn minimize_window(&self, internal_id: &str, minimized: Minimized) -> Result<()> {
        let minimized = minimized == Minimized::Yes;
        self.calls
            .lock()
            .await
            .push(format!("minimized:{internal_id}:{minimized}"));
        Ok(())
    }

    async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()> {
        self.calls
            .lock()
            .await
            .push(format!("foreground:{internal_id}"));
        Ok(())
    }

    async fn move_window_to_current_desktop(&self, internal_id: &str) -> Result<()> {
        self.calls
            .lock()
            .await
            .push(format!("desktop:{internal_id}"));
        Ok(())
    }
}

fn app(name: &str, hotkey: &str, filename: &str) -> AppConfig {
    AppConfig {
        name: name.into(),
        hotkey: Hotkey::parse(hotkey).unwrap(),
        filename: Some(filename.into()),
        command: vec![filename.into()],
        process_name: None,
        window_title: None,
        attach_mode: AttachMode::FindOrStart,
        working_directory: None,
        hide_decorations: false,
        hide_behavior: HideBehavior::Offscreen,
        hide_on_focus_lost: false,
        follow_current_desktop: false,
        placement: PlacementConfig::default(),
        animation: AnimationConfig::default(),
    }
}

fn managed_app(config: AppConfig, tracked_window_id: &str) -> ManagedApp {
    ManagedApp {
        shortcut_id: format!("plasma_drop_hotkey_{}_1", config.sanitized_name()),
        config,
        tracked_window_id: Some(tracked_window_id.into()),
        restore_geometry: None,
        restore_no_border: None,
    }
}

const fn geometry(x: i32, y: i32, width: i32, height: i32) -> FrameGeometry {
    FrameGeometry {
        x,
        y,
        width,
        height,
    }
}

fn window(
    internal_id: &str,
    identity: &str,
    caption: &str,
    frame_geometry: FrameGeometry,
) -> ManagedWindow {
    ManagedWindow {
        internal_id: internal_id.into(),
        desktop_file_name: identity.into(),
        resource_class: identity.into(),
        resource_name: identity.into(),
        caption: caption.into(),
        frame_geometry,
        no_border: false,
    }
}

fn screen() -> ScreenInfo {
    ScreenInfo {
        index: 0,
        name: "screen".into(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    }
}

fn stacked_screens() -> Vec<ScreenInfo> {
    vec![
        ScreenInfo {
            index: 0,
            name: "main".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        ScreenInfo {
            index: 1,
            name: "top".into(),
            x: 0,
            y: -1080,
            width: 1920,
            height: 1080,
        },
    ]
}

fn divided_stale_screens() -> Vec<ScreenInfo> {
    vec![
        ScreenInfo {
            index: 0,
            name: "main-top".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        ScreenInfo {
            index: 1,
            name: "main-bottom".into(),
            x: 0,
            y: 1080,
            width: 1920,
            height: 120,
        },
    ]
}

fn mock_kwin(window: Option<ManagedWindow>) -> Arc<MockKWin> {
    Arc::new(MockKWin {
        window: Mutex::new(window),
        ..MockKWin::default()
    })
}

#[tokio::test]
async fn toggle_on_uses_move_resize_foreground_order() {
    let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "move:{abc}:0:0:1920:1080".to_string(),
            "resize:{abc}:0:0:1920:1080".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_on_moves_to_current_desktop_when_following() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.follow_current_desktop = true;
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "desktop:{abc}".to_string(),
            "move:{abc}:0:0:1920:1080".to_string(),
            "resize:{abc}:0:0:1920:1080".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_on_uses_screen_under_cursor() {
    let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    *kwin.cursor_position.lock().await = Some(Point { x: 500, y: -500 });
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), stacked_screens());

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "move:{abc}:0:-1080:1920:1080".to_string(),
            "resize:{abc}:0:-1080:1920:1080".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_on_prefers_configured_screen_over_cursor() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.placement.screen = Some("top".into());
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    *kwin.cursor_position.lock().await = Some(Point { x: 500, y: 500 });
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), stacked_screens());

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "move:{abc}:0:-1080:1920:1080".to_string(),
            "resize:{abc}:0:-1080:1920:1080".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_on_uses_cursor_screen_when_configured_screen_is_missing() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.placement.screen = Some("HDMI-A-1".into());
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    *kwin.cursor_position.lock().await = Some(Point { x: 500, y: -500 });
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), stacked_screens());

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "move:{abc}:0:-1080:1920:1080".to_string(),
            "resize:{abc}:0:-1080:1920:1080".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_on_refreshes_screens_after_external_display_disconnect() {
    let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    *kwin.cursor_position.lock().await = Some(Point { x: 500, y: 1100 });
    *kwin.screens.lock().await = vec![ScreenInfo {
        index: 0,
        name: "main".into(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1200,
    }];
    let service = ToggleService::new(
        registry,
        Arc::<MockKWin>::clone(&kwin),
        divided_stale_screens(),
    );

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "move:{abc}:0:0:1920:1200".to_string(),
            "resize:{abc}:0:0:1920:1200".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_on_hides_decorations_when_configured() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.hide_decorations = true;
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "no_border:{abc}:true".to_string(),
            "move:{abc}:0:0:1920:1080".to_string(),
            "resize:{abc}:0:0:1920:1080".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
    let restore_no_border = registry
        .lock()
        .await
        .managed_app("dolphin")
        .unwrap()
        .restore_no_border;
    assert_eq!(restore_no_border, Some(false));
}

#[tokio::test]
async fn shutdown_restores_original_decoration_state() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.hide_decorations = true;
    let restore_geometry = geometry(10, 20, 300, 400);
    let mut managed = managed_app(app, "{abc}");
    managed.restore_geometry = Some(restore_geometry);
    managed.restore_no_border = Some(false);
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let mut tracked_window = window("{abc}", "dolphin", "Dolphin", restore_geometry);
    tracked_window.no_border = true;
    let kwin = mock_kwin(Some(tracked_window));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.restore_tracked_windows_on_shutdown().await;

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "move:{abc}:10:20:300:400".to_string(),
            "resize:{abc}:10:20:300:400".to_string(),
            "no_border:{abc}:false".to_string()
        ]
    );
}

#[tokio::test]
async fn different_placements_produce_different_rects() {
    fn make_app(name: &str, hotkey: &str, placement: PlacementConfig) -> ManagedApp {
        let mut app = app(name, hotkey, "irrelevant");
        app.placement = placement;
        managed_app(app, &format!("{{{name}}}"))
    }

    let left = make_app(
        "left",
        "super+f1",
        PlacementConfig {
            width: PlacementMetric::Percent(50),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::Left,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
            screen: None,
        },
    );
    let right = make_app(
        "right",
        "super+f2",
        PlacementConfig {
            width: PlacementMetric::Percent(50),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::Right,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
            screen: None,
        },
    );

    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![left, right])));
    let kwin = mock_kwin(Some(window(
        "placeholder",
        "irrelevant",
        "irrelevant",
        geometry(0, 0, 100, 100),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("left").await.unwrap();
    service.toggle_app("right").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    let move_lines: Vec<_> = calls
        .iter()
        .filter(|line| line.starts_with("move:"))
        .cloned()
        .collect();
    assert!(move_lines.iter().any(|line| line.contains(":0:0:960:1080")));
    assert!(
        move_lines
            .iter()
            .any(|line| line.contains(":960:0:960:1080"))
    );
}

#[tokio::test]
async fn toggle_off_uses_computed_hidden_rect() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.placement = PlacementConfig {
        width: PlacementMetric::Percent(50),
        height: PlacementMetric::Percent(100),
        position: PlacementPosition::Right,
        offset_x: PlacementMetric::Pixels(0),
        offset_y: PlacementMetric::Pixels(0),
        screen: None,
    };
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "resize:{abc}:960:-1080:960:1080".to_string(),
            "move:{abc}:960:-1080:960:1080".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_off_minimizes_when_configured() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.hide_behavior = HideBehavior::Minimize;
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    assert_eq!(
        *kwin.calls.lock().await,
        vec!["minimized:{abc}:true".to_string()]
    );
}

#[tokio::test]
async fn toggle_on_restores_minimized_window() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.hide_behavior = HideBehavior::Minimize;
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    assert_eq!(
        *kwin.calls.lock().await,
        vec![
            "minimized:{abc}:false".to_string(),
            "move:{abc}:0:0:1920:1080".to_string(),
            "resize:{abc}:0:0:1920:1080".to_string(),
            "foreground:{abc}".to_string(),
        ]
    );
}

#[tokio::test]
async fn hides_visible_app_after_focus_moves_away() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.hide_on_focus_lost = true;
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    *kwin.active_window.lock().await = Some(window(
        "{other}",
        "konsole",
        "Konsole",
        geometry(0, 0, 800, 600),
    ));
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service.handle_focus_change(None).await.unwrap();

    assert_eq!(
        *kwin.calls.lock().await,
        vec![
            "resize:{abc}:0:-1080:1920:1080".to_string(),
            "move:{abc}:0:-1080:1920:1080".to_string(),
        ]
    );
    assert_eq!(registry.lock().await.currently_visible_name(), None);
}

#[tokio::test]
async fn focus_change_keeps_the_active_app_visible() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.hide_on_focus_lost = true;
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let active = window("{abc}", "dolphin", "Dolphin", geometry(10, 20, 300, 400));
    let kwin = mock_kwin(Some(active.clone()));
    *kwin.active_window.lock().await = Some(active);
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service.handle_focus_change(None).await.unwrap();

    assert!(kwin.calls.lock().await.is_empty());
    assert_eq!(
        registry.lock().await.currently_visible_name(),
        Some("dolphin")
    );
}

#[tokio::test]
async fn toggle_off_resizes_before_parking_from_taller_screen() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.placement = PlacementConfig {
        width: PlacementMetric::Percent(50),
        height: PlacementMetric::Percent(100),
        position: PlacementPosition::Right,
        offset_x: PlacementMetric::Pixels(0),
        offset_y: PlacementMetric::Pixels(0),
        screen: None,
    };
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(960, 0, 960, 1200),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "resize:{abc}:960:-1080:960:1080".to_string(),
            "move:{abc}:960:-1080:960:1080".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_off_parks_above_all_stacked_screens() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.placement = PlacementConfig {
        width: PlacementMetric::Percent(50),
        height: PlacementMetric::Percent(100),
        position: PlacementPosition::Right,
        offset_x: PlacementMetric::Pixels(0),
        offset_y: PlacementMetric::Pixels(0),
        screen: None,
    };
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(960, 0, 960, 1080),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), stacked_screens());

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "resize:{abc}:960:-2160:960:1080".to_string(),
            "move:{abc}:960:-2160:960:1080".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_on_falls_back_to_active_window_screen() {
    let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    *kwin.active_window.lock().await = Some(window(
        "{active}",
        "konsole",
        "Konsole",
        geometry(100, -1000, 900, 700),
    ));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), stacked_screens());

    service.toggle_app("dolphin").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "move:{abc}:0:-1080:1920:1080".to_string(),
            "resize:{abc}:0:-1080:1920:1080".to_string(),
            "foreground:{abc}".to_string()
        ]
    );
}

#[tokio::test]
async fn toggle_off_recovers_when_tracked_window_id_is_stale() {
    let mut app = app(
        "chromium-flatpak",
        "super+f6",
        "io.github.ungoogled_software.ungoogled_chromium",
    );
    app.command = vec!["/usr/bin/flatpak".into(), "run".into()];
    app.placement = PlacementConfig {
        width: PlacementMetric::Percent(50),
        height: PlacementMetric::Percent(100),
        position: PlacementPosition::Left,
        offset_x: PlacementMetric::Pixels(0),
        offset_y: PlacementMetric::Pixels(0),
        screen: None,
    };
    let managed = managed_app(app, "{stale}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("chromium-flatpak");
    let mut current_window = window(
        "{fresh}",
        "chromium",
        "Ungoogled Chromium",
        geometry(0, 0, 960, 1080),
    );
    current_window.desktop_file_name = "io.github.ungoogled_software.ungoogled_chromium".into();
    let kwin = Arc::new(MockKWin {
        windows: Mutex::new(vec![current_window]),
        ..MockKWin::default()
    });
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service.toggle_app("chromium-flatpak").await.unwrap();

    let calls = kwin.calls.lock().await.clone();
    assert_eq!(
        calls,
        vec![
            "resize:{fresh}:0:-1080:960:1080".to_string(),
            "move:{fresh}:0:-1080:960:1080".to_string()
        ]
    );
    let tracked = registry
        .lock()
        .await
        .managed_app("chromium-flatpak")
        .unwrap()
        .tracked_window_id
        .clone();
    assert_eq!(tracked.as_deref(), Some("{fresh}"));
}

#[tokio::test]
async fn externally_closed_visible_app_is_treated_as_not_visible() {
    let mut app = app("terminal", "super+f6", "kitty");
    app.attach_mode = AttachMode::Find;
    let managed = managed_app(app, "{stale}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("terminal");
    let kwin = Arc::new(MockKWin::default());
    let service = ToggleService::new(Arc::clone(&registry), kwin, vec![screen()]);

    let err = service.toggle_app("terminal").await.unwrap_err();
    assert!(
        err.to_string()
            .contains("no existing window matched app 'terminal'")
    );

    let registry = registry.lock().await;
    let app = registry.managed_app("terminal").unwrap();
    assert_eq!(registry.currently_visible_name(), None);
    assert_eq!(app.tracked_window_id, None);
    drop(registry);
}

fn animation(style: AnimationStyle, duration_ms: u16, frame_delay_ms: u16) -> AnimationConfig {
    AnimationConfig {
        style,
        easing: AnimationEasing::Linear,
        direction: AnimationDirection::Top,
        duration_ms,
        frame_delay_ms,
    }
}

#[tokio::test]
async fn slide_show_resizes_once_and_moves_per_frame() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.animation = animation(AnimationStyle::Slide, 32, 16);
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    assert_eq!(
        *kwin.calls.lock().await,
        vec![
            "move:{abc}:0:-1080:1920:1080".to_string(),
            "resize:{abc}:0:-1080:1920:1080".to_string(),
            "foreground:{abc}".to_string(),
            "move:{abc}:0:-540:1920:1080".to_string(),
            "move:{abc}:0:0:1920:1080".to_string(),
        ]
    );
}

#[tokio::test]
async fn fade_show_applies_final_opacity_once() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.animation = animation(AnimationStyle::Fade, 32, 16);
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.toggle_app("dolphin").await.unwrap();

    let opacity_calls: Vec<_> = kwin
        .calls
        .lock()
        .await
        .iter()
        .filter(|call| call.starts_with("opacity:"))
        .cloned()
        .collect();
    // Setup (0.0) plus one per frame; a fade-in has no teardown opacity.
    assert_eq!(
        opacity_calls,
        vec![
            "opacity:{abc}:0.000".to_string(),
            "opacity:{abc}:0.500".to_string(),
            "opacity:{abc}:1.000".to_string(),
        ]
    );
}

#[tokio::test]
async fn toggle_on_with_other_visible_fetches_screens_once() {
    let first = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let second = managed_app(app("kitty", "super+f8", "kitty"), "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![first, second])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service.toggle_app("kitty").await.unwrap();

    assert_eq!(kwin.info_requests.load(Ordering::Relaxed), 1);
    let registry = registry.lock().await;
    assert_eq!(registry.currently_visible_name(), Some("kitty"));
    drop(registry);
}

#[tokio::test]
async fn shutdown_restore_continues_after_failing_app() {
    let mut broken = managed_app(app("broken", "super+f1", "broken"), "{b1}");
    broken.restore_geometry = Some(geometry(1, 2, 3, 4));
    let mut healthy = managed_app(app("healthy", "super+f2", "healthy"), "{h2}");
    healthy.restore_geometry = Some(geometry(10, 20, 300, 400));
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![broken, healthy])));
    let kwin = Arc::new(MockKWin {
        window: Mutex::new(Some(window(
            "{h2}",
            "healthy",
            "Healthy",
            geometry(0, 0, 100, 100),
        ))),
        fail_window_ids: vec!["{b1}".into()],
        ..MockKWin::default()
    });
    let service = ToggleService::new(registry, Arc::<MockKWin>::clone(&kwin), vec![screen()]);

    service.restore_tracked_windows_on_shutdown().await;

    assert_eq!(
        *kwin.calls.lock().await,
        vec![
            "move:{h2}:10:20:300:400".to_string(),
            "resize:{h2}:10:20:300:400".to_string(),
        ]
    );
}

#[tokio::test]
async fn failed_hide_animation_still_marks_hidden() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.animation = animation(AnimationStyle::Fade, 32, 16);
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = Arc::new(MockKWin {
        window: Mutex::new(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(0, 0, 1920, 1080),
        ))),
        fail_moves: true,
        ..MockKWin::default()
    });
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    assert!(service.toggle_app("dolphin").await.is_err());

    let registry_guard = registry.lock().await;
    assert_eq!(registry_guard.currently_visible_name(), None);
    drop(registry_guard);
}

#[tokio::test]
async fn spawn_polling_error_is_returned() {
    let mut app = app("tool", "super+f3", "true");
    app.attach_mode = AttachMode::FindOrStart;
    let mut managed = managed_app(app, "{g0}");
    managed.tracked_window_id = None;
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    // The first window-list query (before spawning) succeeds empty; polling then fails.
    let kwin = Arc::new(MockKWin {
        fail_window_list_from: Some(1),
        ..MockKWin::default()
    });
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    let error = toggle_and_wait(&service, "tool").await.unwrap_err();

    assert!(error.to_string().contains("window list failed"));
}

#[tokio::test]
async fn toggle_off_resolves_window_once() {
    let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service.toggle_app("dolphin").await.unwrap();

    assert_eq!(kwin.window_requests.load(Ordering::Relaxed), 1);
    assert_eq!(registry.lock().await.currently_visible_name(), None);
}

#[tokio::test]
async fn focus_event_for_tracked_window_skips_query() {
    let mut app = app("dolphin", "super+f9", "dolphin");
    app.hide_on_focus_lost = true;
    let managed = managed_app(app, "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    registry.lock().await.mark_visible("dolphin");
    let kwin = mock_kwin(Some(window(
        "{abc}",
        "dolphin",
        "Dolphin",
        geometry(10, 20, 300, 400),
    )));
    // Were KWin queried, this other active window would make the app hide.
    *kwin.active_window.lock().await = Some(window(
        "{other}",
        "konsole",
        "Konsole",
        geometry(0, 0, 800, 600),
    ));
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    service
        .handle_focus_change(Some("{abc}".to_string()))
        .await
        .unwrap();

    assert!(kwin.calls.lock().await.is_empty());
    assert_eq!(
        registry.lock().await.currently_visible_name(),
        Some("dolphin")
    );
}

#[tokio::test]
async fn find_or_start_spawn_adopts_process() {
    let mut app = app("tool", "super+f3", "true");
    app.attach_mode = AttachMode::FindOrStart;
    let mut managed = managed_app(app, "{g0}");
    managed.tracked_window_id = None;
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
    // The pre-spawn lookup finds nothing; the first poll then fails, ending the toggle early.
    let kwin = Arc::new(MockKWin {
        fail_window_list_from: Some(1),
        ..MockKWin::default()
    });
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    assert!(toggle_and_wait(&service, "tool").await.is_err());

    assert_eq!(*kwin.calls.lock().await, vec!["adopt:tool".to_string()]);
}

/// Toggles `app_name` the way the daemon loop does, but waits for a launch inline.
async fn toggle_and_wait(service: &ToggleService, app_name: &str) -> Result<()> {
    if let Some(launch) = service.toggle_app(app_name).await? {
        let finished = service.wait_for_launch(launch).await;
        service.finish_launch(finished).await?;
    }
    Ok(())
}

/// A `FindOrStart` app with no tracked window, so toggling it spawns its command.
fn spawnable_app() -> ManagedApp {
    let mut app = app("tool", "super+f3", "true");
    app.attach_mode = AttachMode::FindOrStart;
    let mut managed = managed_app(app, "{g0}");
    managed.tracked_window_id = None;
    managed
}

#[tokio::test(start_paused = true)]
async fn find_or_start_shows_window_that_appears_after_spawn() {
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![spawnable_app()])));
    // The pre-spawn lookup and the first poll find nothing; the second poll finds the window.
    let kwin = Arc::new(MockKWin {
        windows: Mutex::new(vec![window(
            "{new}",
            "true",
            "Tool",
            geometry(0, 0, 100, 100),
        )]),
        windows_hidden_for: 2,
        ..MockKWin::default()
    });
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    toggle_and_wait(&service, "tool").await.unwrap();

    assert_eq!(kwin.window_list_calls.load(Ordering::Relaxed), 3);
    assert_eq!(
        *kwin.calls.lock().await,
        vec![
            "adopt:tool".to_string(),
            "move:{new}:0:0:1920:1080".to_string(),
            "resize:{new}:0:0:1920:1080".to_string(),
            "foreground:{new}".to_string(),
        ]
    );
    assert_eq!(registry.lock().await.currently_visible_name(), Some("tool"));
}

#[tokio::test(start_paused = true)]
async fn failed_adopt_still_polls_and_shows_the_window() {
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![spawnable_app()])));
    let kwin = Arc::new(MockKWin {
        windows: Mutex::new(vec![window(
            "{new}",
            "true",
            "Tool",
            geometry(0, 0, 100, 100),
        )]),
        windows_hidden_for: 1,
        fail_adopt: true,
        ..MockKWin::default()
    });
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    toggle_and_wait(&service, "tool").await.unwrap();

    assert_eq!(registry.lock().await.currently_visible_name(), Some("tool"));
    let calls = kwin.calls.lock().await.clone();
    assert_eq!(calls.first().map(String::as_str), Some("adopt:tool"));
    assert!(calls.contains(&"foreground:{new}".to_string()));
}

#[tokio::test(start_paused = true)]
async fn find_or_start_gives_up_when_no_window_appears_in_time() {
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![spawnable_app()])));
    let kwin = Arc::new(MockKWin::default());
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );
    let started = tokio::time::Instant::now();

    let error = toggle_and_wait(&service, "tool").await.unwrap_err();

    assert!(started.elapsed() >= SPAWN_POLL_TIMEOUT);
    assert!(
        error
            .to_string()
            .contains("spawned app 'tool' but no matching window appeared")
    );
    assert_eq!(registry.lock().await.currently_visible_name(), None);
}

#[tokio::test]
async fn press_while_app_is_starting_is_ignored() {
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![spawnable_app()])));
    let kwin = Arc::new(MockKWin::default());
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    let launch = service.toggle_app("tool").await.unwrap();
    let second = service.toggle_app("tool").await.unwrap();

    assert!(launch.is_some());
    assert!(second.is_none());
    // Spawned (and adopted) once; the second press queried nothing.
    assert_eq!(*kwin.calls.lock().await, vec!["adopt:tool".to_string()]);
    assert_eq!(kwin.window_list_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test(start_paused = true)]
async fn other_app_toggles_while_one_is_starting() {
    let dolphin = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}");
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![spawnable_app(), dolphin])));
    // `window` resolves dolphin's tracked window; the list holds only the started tool,
    // hidden from the pre-spawn lookup.
    let kwin = Arc::new(MockKWin {
        windows: Mutex::new(vec![window(
            "{new}",
            "true",
            "Tool",
            geometry(0, 0, 100, 100),
        )]),
        windows_hidden_for: 1,
        window: Mutex::new(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(10, 20, 300, 400),
        ))),
        ..MockKWin::default()
    });
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    let launch = service.toggle_app("tool").await.unwrap().unwrap();
    assert!(service.toggle_app("dolphin").await.unwrap().is_none());
    assert_eq!(
        registry.lock().await.currently_visible_name(),
        Some("dolphin")
    );

    let finished = service.wait_for_launch(launch).await;
    service.finish_launch(finished).await.unwrap();

    assert_eq!(registry.lock().await.currently_visible_name(), Some("tool"));
    assert!(!registry.lock().await.is_launching("tool"));
}

#[tokio::test(start_paused = true)]
async fn app_can_be_started_again_after_a_failed_start() {
    let registry = Arc::new(Mutex::new(AppRegistry::new(vec![spawnable_app()])));
    let kwin = Arc::new(MockKWin::default());
    let service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<MockKWin>::clone(&kwin),
        vec![screen()],
    );

    assert!(toggle_and_wait(&service, "tool").await.is_err());

    assert!(!registry.lock().await.is_launching("tool"));
    assert!(service.toggle_app("tool").await.unwrap().is_some());
}
