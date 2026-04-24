use crate::animation::{TransitionPhase, TransitionPlan, WindowState};
use crate::app_registry::AppRegistry;
use crate::config::{AppConfig, AttachMode};
use crate::screen::ScreenInfo;
use crate::wm::{find_best_match, FrameGeometry, ManagedWindow, WindowManager};
use anyhow::{bail, Context, Result};
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
    screen: ScreenInfo,
    recent_hotkeys: Arc<Mutex<HashMap<String, Instant>>>,
    pending_spawns: Arc<Mutex<HashMap<String, Instant>>>,
}

impl ToggleService {
    pub fn new(
        registry: Arc<Mutex<AppRegistry>>,
        kwin: Arc<dyn WindowManager>,
        screen: ScreenInfo,
    ) -> Self {
        Self {
            registry,
            kwin,
            screen,
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
            self.show_app(app_name).await
        }
    }

    async fn show_app(&self, app_name: &str) -> Result<()> {
        let registry = self.registry.lock().await;
        let app = require_app(&registry, app_name)?;
        let config = app.config.clone();
        let existing_id = app.tracked_window_id.clone();
        drop(registry);

        let window = self.resolve_window(&config, existing_id).await?;
        let visible_rect = self.screen.placement_rect(&config.placement);
        if let Some(plan) = TransitionPlan::from_config(
            &config.animation,
            &self.screen,
            &visible_rect,
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
            let visible_rect = self.screen.placement_rect(&config.placement);
            if let Some(plan) = TransitionPlan::from_config(
                &config.animation,
                &self.screen,
                &visible_rect,
                TransitionPhase::Hide,
            ) {
                self.run_animation(&window.internal_id, &plan, false)
                    .await?;
            } else {
                let hidden_rect = self.screen.hidden_rect_for(&visible_rect);
                self.kwin
                    .move_window(&window.internal_id, &hidden_rect)
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
                        app.restore_geometry.clone()?,
                    ))
                })
                .collect()
        };

        for (app_name, window_id, restore_geometry) in tracked_windows {
            if self.kwin.get_window(&window_id).await?.is_none() {
                continue;
            }

            self.kwin.move_window(&window_id, &restore_geometry).await?;
            self.kwin
                .resize_window(&window_id, &restore_geometry)
                .await?;
            info!(
                "restored app '{}' before shutdown (previous geometry was {}x{} at {},{})",
                app_name,
                restore_geometry.width,
                restore_geometry.height,
                restore_geometry.x,
                restore_geometry.y
            );
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
    use super::{should_ignore_shortcut, ToggleService, HOTKEY_DEBOUNCE_WINDOW};
    use crate::app_registry::{AppRegistry, ManagedApp};
    use crate::config::{
        AnimationConfig, AppConfig, AttachMode, PlacementConfig, PlacementMetric, PlacementPosition,
    };
    use crate::hotkey::Hotkey;
    use crate::screen::ScreenInfo;
    use crate::wm::{FrameGeometry, ManagedWindow, WindowManager};
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

        async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()> {
            self.calls
                .lock()
                .await
                .push(format!("foreground:{internal_id}"));
            Ok(())
        }
    }

    #[tokio::test]
    async fn toggle_on_uses_move_resize_foreground_order() {
        let app = AppConfig {
            name: "dolphin".into(),
            hotkey: Hotkey::parse("super+f9").unwrap(),
            filename: Some("dolphin".into()),
            command: vec!["dolphin".into()],
            process_name: None,
            window_title: None,
            attach_mode: AttachMode::FindOrStart,
            working_directory: None,
            placement: PlacementConfig::default(),
            animation: AnimationConfig::default(),
        };
        let managed = ManagedApp {
            config: app,
            tracked_window_id: Some("{abc}".into()),
            restore_geometry: None,
            visible: false,
            shortcut_id: "plasma_drop_hotkey_dolphin_1".into(),
        };
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(Some(ManagedWindow {
                internal_id: "{abc}".into(),
                desktop_file_name: "dolphin".into(),
                resource_class: "dolphin".into(),
                resource_name: "dolphin".into(),
                caption: "Dolphin".into(),
                frame_geometry: FrameGeometry {
                    x: 10,
                    y: 20,
                    width: 300,
                    height: 400,
                },
            })),
            windows: Mutex::new(Vec::new()),
        });
        let service = ToggleService::new(
            registry,
            kwin.clone(),
            ScreenInfo {
                index: 0,
                name: "screen".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        );

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
    async fn different_placements_produce_different_rects() {
        fn make_app(name: &str, hotkey: &str, placement: PlacementConfig) -> ManagedApp {
            ManagedApp {
                config: AppConfig {
                    name: name.into(),
                    hotkey: Hotkey::parse(hotkey).unwrap(),
                    filename: Some("irrelevant".into()),
                    command: vec!["irrelevant".into()],
                    process_name: None,
                    window_title: None,
                    attach_mode: AttachMode::FindOrStart,
                    working_directory: None,
                    placement,
                    animation: AnimationConfig::default(),
                },
                tracked_window_id: Some(format!("{{{name}}}")),
                restore_geometry: None,
                visible: false,
                shortcut_id: format!("plasma_drop_hotkey_{name}_1"),
            }
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
        let kwin = Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(Some(ManagedWindow {
                internal_id: "placeholder".into(),
                desktop_file_name: "irrelevant".into(),
                resource_class: "irrelevant".into(),
                resource_name: "irrelevant".into(),
                caption: "irrelevant".into(),
                frame_geometry: FrameGeometry {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
            })),
            windows: Mutex::new(Vec::new()),
        });
        let service = ToggleService::new(
            registry,
            kwin.clone(),
            ScreenInfo {
                index: 0,
                name: "screen".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        );

        service.toggle_app("left").await.unwrap();
        service.toggle_app("right").await.unwrap();

        let calls = kwin.calls.lock().await.clone();
        let move_lines: Vec<_> = calls
            .iter()
            .filter(|line| line.starts_with("move:"))
            .cloned()
            .collect();
        assert!(move_lines.iter().any(|line| line.contains(":0:0:960:1080")));
        assert!(move_lines
            .iter()
            .any(|line| line.contains(":960:0:960:1080")));
    }

    #[tokio::test]
    async fn toggle_off_uses_computed_hidden_rect() {
        let app = AppConfig {
            name: "dolphin".into(),
            hotkey: Hotkey::parse("super+f9").unwrap(),
            filename: Some("dolphin".into()),
            command: vec!["dolphin".into()],
            process_name: None,
            window_title: None,
            attach_mode: AttachMode::FindOrStart,
            working_directory: None,
            placement: PlacementConfig {
                width: PlacementMetric::Percent(50),
                height: PlacementMetric::Percent(100),
                position: PlacementPosition::Right,
                offset_x: PlacementMetric::Pixels(0),
                offset_y: PlacementMetric::Pixels(0),
            },
            animation: AnimationConfig::default(),
        };
        let managed = ManagedApp {
            config: app,
            tracked_window_id: Some("{abc}".into()),
            restore_geometry: None,
            visible: true,
            shortcut_id: "plasma_drop_hotkey_dolphin_1".into(),
        };
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(Some(ManagedWindow {
                internal_id: "{abc}".into(),
                desktop_file_name: "dolphin".into(),
                resource_class: "dolphin".into(),
                resource_name: "dolphin".into(),
                caption: "Dolphin".into(),
                frame_geometry: FrameGeometry {
                    x: 10,
                    y: 20,
                    width: 300,
                    height: 400,
                },
            })),
            windows: Mutex::new(Vec::new()),
        });
        let service = ToggleService::new(
            registry,
            kwin.clone(),
            ScreenInfo {
                index: 0,
                name: "screen".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        );

        service.toggle_app("dolphin").await.unwrap();

        let calls = kwin.calls.lock().await.clone();
        assert_eq!(calls, vec!["move:{abc}:960:-1080:960:1080".to_string()]);
    }

    #[tokio::test]
    async fn toggle_off_recovers_when_tracked_window_id_is_stale() {
        let app = AppConfig {
            name: "chromium-flatpak".into(),
            hotkey: Hotkey::parse("super+f6").unwrap(),
            filename: Some("io.github.ungoogled_software.ungoogled_chromium".into()),
            command: vec!["/usr/bin/flatpak".into(), "run".into()],
            process_name: None,
            window_title: None,
            attach_mode: AttachMode::FindOrStart,
            working_directory: None,
            placement: PlacementConfig {
                width: PlacementMetric::Percent(50),
                height: PlacementMetric::Percent(100),
                position: PlacementPosition::Left,
                offset_x: PlacementMetric::Pixels(0),
                offset_y: PlacementMetric::Pixels(0),
            },
            animation: AnimationConfig::default(),
        };
        let managed = ManagedApp {
            config: app,
            tracked_window_id: Some("{stale}".into()),
            restore_geometry: None,
            visible: true,
            shortcut_id: "plasma_drop_hotkey_chromium_flatpak_1".into(),
        };
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let current_window = ManagedWindow {
            internal_id: "{fresh}".into(),
            desktop_file_name: "io.github.ungoogled_software.ungoogled_chromium".into(),
            resource_class: "chromium".into(),
            resource_name: "chromium".into(),
            caption: "Ungoogled Chromium".into(),
            frame_geometry: FrameGeometry {
                x: 0,
                y: 0,
                width: 960,
                height: 1080,
            },
        };
        let kwin = Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(None),
            windows: Mutex::new(vec![current_window]),
        });
        let service = ToggleService::new(
            registry.clone(),
            kwin.clone(),
            ScreenInfo {
                index: 0,
                name: "screen".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        );

        service.toggle_app("chromium-flatpak").await.unwrap();

        let calls = kwin.calls.lock().await.clone();
        assert_eq!(calls, vec!["move:{fresh}:0:-1080:960:1080".to_string()]);
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
        let app = AppConfig {
            name: "terminal".into(),
            hotkey: Hotkey::parse("super+f6").unwrap(),
            filename: Some("kitty".into()),
            command: vec!["kitty".into()],
            process_name: None,
            window_title: None,
            attach_mode: AttachMode::Find,
            working_directory: None,
            placement: PlacementConfig::default(),
            animation: AnimationConfig::default(),
        };
        let managed = ManagedApp {
            config: app,
            tracked_window_id: Some("{stale}".into()),
            restore_geometry: None,
            visible: true,
            shortcut_id: "plasma_drop_hotkey_terminal_1".into(),
        };
        let registry = Arc::new(Mutex::new(AppRegistry::new(vec![managed])));
        let kwin = Arc::new(MockKWin {
            calls: Mutex::new(Vec::new()),
            window: Mutex::new(None),
            windows: Mutex::new(Vec::new()),
        });
        let service = ToggleService::new(
            registry.clone(),
            kwin,
            ScreenInfo {
                index: 0,
                name: "screen".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
        );

        let err = service.toggle_app("terminal").await.unwrap_err();
        assert!(err
            .to_string()
            .contains("no existing window matched app 'terminal'"));

        let registry = registry.lock().await;
        let app = registry.managed_app("terminal").unwrap();
        assert!(!app.visible);
        assert_eq!(app.tracked_window_id, None);
        drop(registry);
    }
}
