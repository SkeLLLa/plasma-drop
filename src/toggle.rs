use crate::animation::{TransitionPhase, TransitionPlan, WindowState};
use crate::app_registry::AppRegistry;
use crate::config::{AppConfig, AttachMode};
use crate::screen::{ScreenInfo, hidden_rect_for_screens};
use crate::wm::{FrameGeometry, ManagedWindow, Point, WindowManager, find_best_match};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{info, warn};

const HOTKEY_DEBOUNCE_WINDOW: Duration = Duration::from_millis(400);
const SPAWN_POLL_ATTEMPTS: u32 = 40;
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(250);
const SPAWN_GATE_TTL: Duration = Duration::from_secs(15);

fn require_app<'r>(
    registry: &'r AppRegistry,
    name: &str,
) -> Result<&'r crate::app_registry::ManagedApp> {
    registry
        .managed_app(name)
        .with_context(|| format!("unknown app '{name}'"))
}

#[derive(Clone)]
pub struct ToggleService {
    registry: Arc<Mutex<AppRegistry>>,
    kwin: Arc<dyn WindowManager>,
    screens: Vec<ScreenInfo>,
    recent_hotkeys: Arc<Mutex<HashMap<String, Instant>>>,
    pending_spawns: Arc<Mutex<HashMap<String, Instant>>>,
}

impl ToggleService {
    pub fn new(
        registry: Arc<Mutex<AppRegistry>>,
        kwin: Arc<dyn WindowManager>,
        screens: Vec<ScreenInfo>,
    ) -> Self {
        assert!(
            !screens.is_empty(),
            "ToggleService requires at least one screen"
        );
        Self {
            registry,
            kwin,
            screens,
            recent_hotkeys: Arc::new(Mutex::new(HashMap::new())),
            pending_spawns: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn handle_shortcut(&self, shortcut_id: &str) -> Result<()> {
        if self.should_ignore_shortcut(shortcut_id).await {
            info!("ignored repeated hotkey '{}'", shortcut_id);
            return Ok(());
        }

        let app_name = {
            let registry = self.registry.lock().await;
            registry
                .app_for_shortcut(shortcut_id)
                .map(str::to_string)
                .with_context(|| format!("unknown shortcut '{shortcut_id}'"))?
        };

        self.toggle_app(&app_name).await
    }

    async fn should_ignore_shortcut(&self, shortcut_id: &str) -> bool {
        let now = Instant::now();
        let mut recent_hotkeys = self.recent_hotkeys.lock().await;
        should_ignore_shortcut(&mut recent_hotkeys, shortcut_id, now)
    }

    pub async fn toggle_app(&self, app_name: &str) -> Result<()> {
        let registry = self.registry.lock().await;
        let other_visible = registry
            .currently_visible_name()
            .filter(|name| *name != app_name)
            .map(str::to_string);
        let target = require_app(&registry, app_name)?;
        let target_visible = target.visible;
        let target_config = target.config.clone();
        let target_tracked_window_id = target.tracked_window_id.clone();
        drop(registry);

        let screen_for_show = if target_visible {
            None
        } else {
            Some(self.current_screen().await?.clone())
        };

        if let Some(other) = other_visible {
            self.hide_app(&other).await?;
        }

        let target_visible = if target_visible {
            let window = self
                .resolve_existing_window(&target_config, target_tracked_window_id)
                .await?;
            if let Some(window) = window {
                let mut registry = self.registry.lock().await;
                if let Some(app) = registry.managed_app_mut(app_name) {
                    app.tracked_window_id = Some(window.internal_id);
                    app.visible = true;
                }
                drop(registry);
                true
            } else {
                let mut registry = self.registry.lock().await;
                if let Some(app) = registry.managed_app_mut(app_name) {
                    app.tracked_window_id = None;
                }
                registry.set_visible(app_name, false);
                drop(registry);
                false
            }
        } else {
            false
        };

        if target_visible {
            self.hide_app(app_name).await
        } else {
            self.show_app(app_name, screen_for_show).await
        }
    }

    async fn show_app(&self, app_name: &str, preferred_screen: Option<ScreenInfo>) -> Result<()> {
        let registry = self.registry.lock().await;
        let app = require_app(&registry, app_name)?;
        let config = app.config.clone();
        let existing_id = app.tracked_window_id.clone();
        drop(registry);

        let window = self.resolve_window(&config, existing_id).await?;
        if config.hide_decorations {
            self.hide_window_decorations(app_name, &window).await?;
        }

        let screen = match preferred_screen {
            Some(screen) => screen,
            None => self.current_screen().await?.clone(),
        };
        screen
            .validate_placement(&config.placement)
            .with_context(|| format!("invalid placement for '{app_name}'"))?;
        let visible_rect = screen.placement_rect(&config.placement);
        let hidden_rect = hidden_rect_for_screens(&self.screens, &visible_rect);
        if let Some(plan) = TransitionPlan::from_config(
            &config.animation,
            &visible_rect,
            &hidden_rect,
            TransitionPhase::Show,
        ) {
            self.run_animation(&window.internal_id, &plan, true).await?;
        } else {
            self.apply_geometry(&window.internal_id, &visible_rect)
                .await?;
            self.kwin
                .bring_window_to_foreground(&window.internal_id)
                .await?;
        }

        let mut registry = self.registry.lock().await;
        let app = registry
            .managed_app_mut(app_name)
            .with_context(|| format!("unknown app '{app_name}'"))?;
        let is_new_window = app.tracked_window_id.as_deref() != Some(window.internal_id.as_str());
        if is_new_window || app.restore_geometry.is_none() {
            app.restore_geometry = Some(window.frame_geometry.clone());
        }
        app.tracked_window_id = Some(window.internal_id.clone());
        registry.set_visible(app_name, true);
        drop(registry);
        info!("showed app '{app_name}'");
        Ok(())
    }

    async fn hide_app(&self, app_name: &str) -> Result<()> {
        let registry = self.registry.lock().await;
        let app = require_app(&registry, app_name)?;
        let config = app.config.clone();
        let tracked_window_id = app.tracked_window_id.clone();
        drop(registry);

        let resolved_window = self
            .resolve_existing_window(&config, tracked_window_id)
            .await?;
        if let Some(window) = resolved_window {
            let screen = self.screen_for_window(&window);
            let visible_rect = screen.placement_rect(&config.placement);
            let hidden_rect = hidden_rect_for_screens(&self.screens, &visible_rect);
            if let Some(plan) = TransitionPlan::from_config(
                &config.animation,
                &visible_rect,
                &hidden_rect,
                TransitionPhase::Hide,
            ) {
                self.run_animation(&window.internal_id, &plan, false)
                    .await?;
            } else {
                self.apply_hidden_geometry(&window.internal_id, &hidden_rect)
                    .await?;
            }

            let mut registry = self.registry.lock().await;
            let app = registry
                .managed_app_mut(app_name)
                .with_context(|| format!("unknown app '{app_name}'"))?;
            app.tracked_window_id = Some(window.internal_id);
            drop(registry);
        } else {
            warn!(
                "cannot hide app '{}' because no tracked window is attached",
                app_name
            );
        }

        self.registry.lock().await.set_visible(app_name, false);
        info!("hid app '{app_name}'");
        Ok(())
    }

    async fn run_animation(
        &self,
        internal_id: &str,
        plan: &TransitionPlan,
        bring_to_front: bool,
    ) -> Result<()> {
        self.apply_window_state(internal_id, plan.setup_state())
            .await?;
        if bring_to_front {
            self.kwin.bring_window_to_foreground(internal_id).await?;
        }

        let frame_count = plan.frame_count();
        let frame_delay = Duration::from_millis(16);
        for frame_idx in 1..=frame_count {
            self.apply_window_state(internal_id, plan.frame_state(frame_idx))
                .await?;
            if frame_idx < frame_count {
                tokio::time::sleep(frame_delay).await;
            }
        }

        self.apply_window_state(internal_id, plan.final_state())
            .await?;
        self.apply_window_state(internal_id, plan.teardown_state())
            .await?;
        Ok(())
    }

    async fn apply_window_state(&self, internal_id: &str, state: WindowState) -> Result<()> {
        if let Some(geometry) = state.geometry {
            self.apply_geometry(internal_id, &geometry).await?;
        }
        if let Some(opacity) = state.opacity {
            self.kwin.set_window_opacity(internal_id, opacity).await?;
        }
        Ok(())
    }

    async fn apply_geometry(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        self.kwin.move_window(internal_id, geometry).await?;
        self.kwin.resize_window(internal_id, geometry).await?;
        Ok(())
    }

    async fn apply_hidden_geometry(
        &self,
        internal_id: &str,
        geometry: &FrameGeometry,
    ) -> Result<()> {
        self.kwin.resize_window(internal_id, geometry).await?;
        self.kwin.move_window(internal_id, geometry).await?;
        Ok(())
    }

    async fn current_screen(&self) -> Result<&ScreenInfo> {
        if let Some(position) = self.kwin.get_cursor_position().await? {
            if let Some(screen) = self.screen_containing_point(&position) {
                return Ok(screen);
            }
        }

        if let Some(window) = self.kwin.get_active_window().await? {
            return Ok(self.screen_for_geometry(&window.frame_geometry));
        }

        Ok(&self.screens[0])
    }

    fn screen_for_window(&self, window: &ManagedWindow) -> &ScreenInfo {
        self.screen_for_geometry(&window.frame_geometry)
    }

    fn screen_containing_point(&self, position: &Point) -> Option<&ScreenInfo> {
        self.screens
            .iter()
            .find(|screen| screen.contains_point(position.x, position.y))
    }

    fn screen_for_geometry(&self, geometry: &FrameGeometry) -> &ScreenInfo {
        self.screens
            .iter()
            .max_by_key(|screen| screen.overlap_area(geometry))
            .unwrap_or(&self.screens[0])
    }

    async fn hide_window_decorations(&self, app_name: &str, window: &ManagedWindow) -> Result<()> {
        let mut registry = self.registry.lock().await;
        let app = registry
            .managed_app_mut(app_name)
            .with_context(|| format!("unknown app '{app_name}'"))?;
        let is_new_window = app.tracked_window_id.as_deref() != Some(window.internal_id.as_str());
        if is_new_window || app.restore_no_border.is_none() {
            app.restore_no_border = Some(window.no_border);
        }
        drop(registry);

        if !window.no_border {
            self.kwin
                .set_window_no_border(&window.internal_id, true)
                .await?;
        }

        Ok(())
    }

    async fn resolve_window(
        &self,
        config: &AppConfig,
        tracked_window_id: Option<String>,
    ) -> Result<ManagedWindow> {
        if let Some(window) = self
            .resolve_existing_window(config, tracked_window_id)
            .await?
        {
            return Ok(window);
        }

        match config.attach_mode {
            AttachMode::Find => bail!("no existing window matched app '{}'", config.name),
            AttachMode::FindOrStart => {
                let claimed = self.claim_spawn_gate(&config.name).await;
                if claimed {
                    if let Err(error) = Self::spawn_app(config) {
                        self.release_spawn_gate(&config.name).await;
                        return Err(error);
                    }
                } else {
                    info!(
                        "spawn for '{}' already in progress; polling for window",
                        config.name
                    );
                }

                let mut attached = None;
                for _ in 0..SPAWN_POLL_ATTEMPTS {
                    tokio::time::sleep(SPAWN_POLL_INTERVAL).await;
                    if let Some(window) = self.find_matching_window(config).await? {
                        attached = Some(window);
                        break;
                    }
                }

                if claimed {
                    self.release_spawn_gate(&config.name).await;
                }

                match attached {
                    Some(window) => Ok(window),
                    None => bail!(
                        "spawned app '{}' but no matching window appeared",
                        config.name
                    ),
                }
            }
        }
    }

    async fn claim_spawn_gate(&self, app_name: &str) -> bool {
        let now = Instant::now();
        let mut pending = self.pending_spawns.lock().await;
        pending.retain(|_, instant| now.duration_since(*instant) < SPAWN_GATE_TTL);
        if pending.contains_key(app_name) {
            false
        } else {
            pending.insert(app_name.to_string(), now);
            true
        }
    }

    async fn release_spawn_gate(&self, app_name: &str) {
        self.pending_spawns.lock().await.remove(app_name);
    }

    async fn resolve_existing_window(
        &self,
        config: &AppConfig,
        tracked_window_id: Option<String>,
    ) -> Result<Option<ManagedWindow>> {
        if let Some(window_id) = tracked_window_id {
            if let Some(window) = self.kwin.get_window(&window_id).await? {
                return Ok(Some(window));
            }
        }

        if let Some(window) = self.find_matching_window(config).await? {
            return Ok(Some(window));
        }

        Ok(None)
    }

    async fn find_matching_window(&self, config: &AppConfig) -> Result<Option<ManagedWindow>> {
        let windows = self.kwin.get_window_list().await?;
        let (window, count) = find_best_match(&windows, config);
        if count > 1 {
            warn!(
                "multiple windows matched app '{}'; using the first result",
                config.name
            );
        }
        Ok(window.cloned())
    }

    fn spawn_app(config: &AppConfig) -> Result<()> {
        let (program, arguments) = config.command.split_first().with_context(|| {
            format!(
                "app '{}' cannot spawn without filename or command",
                config.name
            )
        })?;

        let mut command = Command::new(program);
        command.args(arguments);
        if let Some(dir) = config.working_directory.as_ref() {
            command.current_dir(dir);
        }

        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        command
            .spawn()
            .with_context(|| format!("failed to spawn '{program}'"))?;
        info!("spawned app '{}'", config.name);
        Ok(())
    }

    pub async fn restore_tracked_windows_on_shutdown(&self) -> Result<()> {
        let tracked_windows: Vec<_> = {
            let registry = self.registry.lock().await;
            registry
                .managed_apps()
                .filter_map(|app| {
                    Some((
                        app.config.name.clone(),
                        app.tracked_window_id.clone()?,
                        app.restore_geometry.clone(),
                        app.restore_no_border,
                    ))
                })
                .collect()
        };

        for (app_name, window_id, restore_geometry, restore_no_border) in tracked_windows {
            if self.kwin.get_window(&window_id).await?.is_none() {
                continue;
            }

            if let Some(restore_geometry) = restore_geometry {
                self.kwin.move_window(&window_id, &restore_geometry).await?;
                self.kwin
                    .resize_window(&window_id, &restore_geometry)
                    .await?;
                info!(
                    "restored app '{}' geometry before shutdown (previous geometry was {}x{} at {},{})",
                    app_name,
                    restore_geometry.width,
                    restore_geometry.height,
                    restore_geometry.x,
                    restore_geometry.y
                );
            }

            if let Some(no_border) = restore_no_border {
                self.kwin
                    .set_window_no_border(&window_id, no_border)
                    .await?;
                info!(
                    "restored app '{}' decoration state before shutdown (no_border={})",
                    app_name, no_border
                );
            }
        }

        Ok(())
    }
}

fn should_ignore_shortcut(
    recent_hotkeys: &mut HashMap<String, Instant>,
    shortcut_id: &str,
    now: Instant,
) -> bool {
    recent_hotkeys.retain(|_, instant| now.duration_since(*instant) <= HOTKEY_DEBOUNCE_WINDOW);

    match recent_hotkeys.get(shortcut_id) {
        Some(previous) if now.duration_since(*previous) <= HOTKEY_DEBOUNCE_WINDOW => true,
        _ => {
            recent_hotkeys.insert(shortcut_id.to_string(), now);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HOTKEY_DEBOUNCE_WINDOW, ToggleService, should_ignore_shortcut};
    use crate::app_registry::{AppRegistry, ManagedApp};
    use crate::config::{
        AnimationConfig, AppConfig, AttachMode, PlacementConfig, PlacementMetric, PlacementPosition,
    };
    use crate::hotkey::Hotkey;
    use crate::screen::ScreenInfo;
    use crate::wm::{FrameGeometry, ManagedWindow, Point, WindowManager};
    use anyhow::Result;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;
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

    #[derive(Default)]
    struct MockKWin {
        calls: Mutex<Vec<String>>,
        window: Mutex<Option<ManagedWindow>>,
        active_window: Mutex<Option<ManagedWindow>>,
        cursor_position: Mutex<Option<Point>>,
        windows: Mutex<Vec<ManagedWindow>>,
    }

    #[async_trait]
    impl WindowManager for MockKWin {
        async fn get_window_list(&self) -> Result<Vec<ManagedWindow>> {
            Ok(self.windows.lock().await.clone())
        }

        async fn get_window(&self, _internal_id: &str) -> Result<Option<ManagedWindow>> {
            Ok(self.window.lock().await.clone())
        }

        async fn get_active_window(&self) -> Result<Option<ManagedWindow>> {
            Ok(self.active_window.lock().await.clone())
        }

        async fn get_cursor_position(&self) -> Result<Option<Point>> {
            Ok(self.cursor_position.lock().await.clone())
        }

        async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
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

        async fn set_window_opacity(&self, internal_id: &str, opacity: f64) -> Result<()> {
            self.calls
                .lock()
                .await
                .push(format!("opacity:{internal_id}:{opacity:.3}"));
            Ok(())
        }

        async fn set_window_no_border(&self, internal_id: &str, no_border: bool) -> Result<()> {
            self.calls
                .lock()
                .await
                .push(format!("no_border:{internal_id}:{no_border}"));
            Ok(())
        }

        async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()> {
            self.calls
                .lock()
                .await
                .push(format!("foreground:{internal_id}"));
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
            placement: PlacementConfig::default(),
            animation: AnimationConfig::default(),
        }
    }

    fn managed_app(config: AppConfig, tracked_window_id: &str, visible: bool) -> ManagedApp {
        ManagedApp {
            shortcut_id: format!("plasma_drop_hotkey_{}_1", config.sanitized_name()),
            config,
            tracked_window_id: Some(tracked_window_id.into()),
            restore_geometry: None,
            restore_no_border: None,
            visible,
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

    fn mock_kwin(window: Option<ManagedWindow>) -> Arc<MockKWin> {
        Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(window),
            active_window: Mutex::new(None),
            cursor_position: Mutex::new(None),
            windows: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn toggle_on_uses_move_resize_foreground_order() {
        let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}", false);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = mock_kwin(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(10, 20, 300, 400),
        )));
        let service = ToggleService::new(registry, kwin.clone(), vec![screen()]);

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
    async fn toggle_on_uses_screen_under_cursor() {
        let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}", false);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = mock_kwin(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(10, 20, 300, 400),
        )));
        *kwin.cursor_position.lock().await = Some(Point { x: 500, y: -500 });
        let service = ToggleService::new(registry, kwin.clone(), stacked_screens());

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
    async fn toggle_on_hides_decorations_when_configured() {
        let mut app = app("dolphin", "super+f9", "dolphin");
        app.hide_decorations = true;
        let managed = managed_app(app, "{abc}", false);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = mock_kwin(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(10, 20, 300, 400),
        )));
        let service = ToggleService::new(registry.clone(), kwin.clone(), vec![screen()]);

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
        let mut managed = managed_app(app, "{abc}", false);
        managed.restore_geometry = Some(restore_geometry.clone());
        managed.restore_no_border = Some(false);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let mut tracked_window = window("{abc}", "dolphin", "Dolphin", restore_geometry);
        tracked_window.no_border = true;
        let kwin = mock_kwin(Some(tracked_window));
        let service = ToggleService::new(registry, kwin.clone(), vec![screen()]);

        service.restore_tracked_windows_on_shutdown().await.unwrap();

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
            managed_app(app, &format!("{{{name}}}"), false)
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
            },
        );

        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![left, right])));
        let kwin = mock_kwin(Some(window(
            "placeholder",
            "irrelevant",
            "irrelevant",
            geometry(0, 0, 100, 100),
        )));
        let service = ToggleService::new(registry, kwin.clone(), vec![screen()]);

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
        };
        let managed = managed_app(app, "{abc}", true);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = mock_kwin(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(10, 20, 300, 400),
        )));
        let service = ToggleService::new(registry, kwin.clone(), vec![screen()]);

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
    async fn toggle_off_resizes_before_parking_from_taller_screen() {
        let mut app = app("dolphin", "super+f9", "dolphin");
        app.placement = PlacementConfig {
            width: PlacementMetric::Percent(50),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::Right,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
        };
        let managed = managed_app(app, "{abc}", true);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = mock_kwin(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(960, 0, 960, 1200),
        )));
        let service = ToggleService::new(registry, kwin.clone(), vec![screen()]);

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
        };
        let managed = managed_app(app, "{abc}", true);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = mock_kwin(Some(window(
            "{abc}",
            "dolphin",
            "Dolphin",
            geometry(960, 0, 960, 1080),
        )));
        let service = ToggleService::new(registry, kwin.clone(), stacked_screens());

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
        let managed = managed_app(app("dolphin", "super+f9", "dolphin"), "{abc}", false);
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
        let service = ToggleService::new(registry, kwin.clone(), stacked_screens());

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
        };
        let managed = managed_app(app, "{stale}", true);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let mut current_window = window(
            "{fresh}",
            "chromium",
            "Ungoogled Chromium",
            geometry(0, 0, 960, 1080),
        );
        current_window.desktop_file_name = "io.github.ungoogled_software.ungoogled_chromium".into();
        let kwin = Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(None),
            active_window: Mutex::new(None),
            cursor_position: Mutex::new(None),
            windows: Mutex::new(vec![current_window]),
        });
        let service = ToggleService::new(registry.clone(), kwin.clone(), vec![screen()]);

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
        let managed = managed_app(app, "{stale}", true);
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(None),
            active_window: Mutex::new(None),
            cursor_position: Mutex::new(None),
            windows: Mutex::new(Vec::new()),
        });
        let service = ToggleService::new(registry.clone(), kwin, vec![screen()]);

        let err = service.toggle_app("terminal").await.unwrap_err();
        assert!(
            err.to_string()
                .contains("no existing window matched app 'terminal'")
        );

        let registry = registry.lock().await;
        let app = registry.managed_app("terminal").unwrap();
        assert!(!app.visible);
        assert_eq!(app.tracked_window_id, None);
        drop(registry);
    }
}
