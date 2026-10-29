use crate::animation::{TransitionPhase, TransitionPlan, WindowState};
use crate::app_registry::{AppRegistry, ManagedApp, RestorePlacement, RestoreState};
use crate::config::{AppConfig, AttachMode, HideBehavior};
use crate::screen::{ScreenInfo, hidden_rect_for_screens};
use crate::wm::{
    Decorations, FrameGeometry, ManagedWindow, Minimized, ModesLeft, Point, WindowManager,
    WindowUpdate, find_best_match,
};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

mod spawn;

const HOTKEY_DEBOUNCE_WINDOW: Duration = Duration::from_millis(400);
/// How long to wait for a spawned app's window before giving up.
const SPAWN_POLL_TIMEOUT: Duration = Duration::from_secs(10);
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// How long to wait for an app to acknowledge leaving maximized or fullscreen state.
const NORMAL_STATE_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a just-launched app gets to settle before an instant show re-applies the
/// placement; an animated show re-applies it when it ends.
const LAUNCH_SETTLE_DELAY: Duration = Duration::from_millis(150);

/// A `find_or_start` app whose command was spawned; its window has not appeared yet.
/// Hand it to [`ToggleService::wait_for_launch`] off the event loop.
#[derive(Debug)]
#[must_use = "the launched app is shown only once its window is awaited"]
pub struct PendingLaunch {
    app_name: String,
    config: AppConfig,
}

/// The outcome of waiting for a launched app's window; hand it to
/// [`ToggleService::finish_launch`] on the event loop.
#[derive(Debug)]
pub struct FinishedLaunch {
    app_name: String,
    window: Result<Option<ManagedWindow>>,
}

fn require_app<'r>(registry: &'r AppRegistry, name: &str) -> Result<&'r ManagedApp> {
    registry
        .managed_app(name)
        .with_context(|| format!("unknown app '{name}'"))
}

fn require_app_mut<'r>(registry: &'r mut AppRegistry, name: &str) -> Result<&'r mut ManagedApp> {
    registry
        .managed_app_mut(name)
        .with_context(|| format!("unknown app '{name}'"))
}

/// How a show places its window.
enum ShowPlacement {
    /// Animate or move the window into `visible`, starting from `hidden`.
    Rects {
        visible: FrameGeometry,
        hidden: FrameGeometry,
    },
    /// Keep the window fullscreen, on `screen`.
    KeepFullScreen { screen: ScreenInfo },
}

#[derive(Clone)]
pub struct ToggleService {
    registry: Arc<Mutex<AppRegistry>>,
    kwin: Arc<dyn WindowManager>,
    screens: Arc<Mutex<Vec<ScreenInfo>>>,
    recent_hotkeys: Arc<Mutex<HashMap<String, Instant>>>,
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
            screens: Arc::new(Mutex::new(screens)),
            recent_hotkeys: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Toggles the app bound to `shortcut_id`; `pressed_at` is when the press reached the
    /// daemon, so presses that queued up behind a slow toggle are debounced by when they
    /// happened. Returns the launch to wait for when the press spawned the app.
    pub async fn handle_shortcut(
        &self,
        shortcut_id: &str,
        pressed_at: Instant,
    ) -> Result<Option<PendingLaunch>> {
        if self.should_ignore_shortcut(shortcut_id, pressed_at).await {
            info!("ignored repeated hotkey '{}'", shortcut_id);
            return Ok(None);
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

    async fn should_ignore_shortcut(&self, shortcut_id: &str, pressed_at: Instant) -> bool {
        let mut recent_hotkeys = self.recent_hotkeys.lock().await;
        should_ignore_shortcut(&mut recent_hotkeys, shortcut_id, pressed_at)
    }

    /// Shows `app_name`, hiding whichever app is visible, or hides it when it is the visible
    /// one. Returns the launch to wait for when the app had no window and was spawned.
    pub async fn toggle_app(&self, app_name: &str) -> Result<Option<PendingLaunch>> {
        let registry = self.registry.lock().await;
        if registry.is_launching(app_name) {
            info!("app '{app_name}' is still starting; ignoring the hotkey");
            return Ok(None);
        }
        let other_visible = registry
            .currently_visible_name()
            .filter(|name| *name != app_name)
            .map(str::to_string);
        let target = require_app(&registry, app_name)?;
        let target_visible = registry.currently_visible_name() == Some(app_name);
        let target_config = target.config.clone();
        let target_tracked_window_id = target.tracked_window_id.clone();
        drop(registry);

        let screens = self.current_screens().await;
        let screen_for_show = if target_visible {
            None
        } else {
            Some(self.current_screen(&target_config, &screens).await?)
        };

        if let Some(other) = other_visible {
            self.hide_app(&other, &screens).await?;
        }

        if target_visible {
            let window = self
                .resolve_existing_window(&target_config, target_tracked_window_id)
                .await?;
            if let Some(window) = window {
                self.hide_resolved(app_name, &target_config, window, &screens)
                    .await?;
                return Ok(None);
            }
            // The window was closed behind our back: forget it and show a fresh one.
            let mut registry = self.registry.lock().await;
            registry.forget_window(app_name);
            registry.mark_hidden(app_name);
        }

        self.show_app(app_name, &screens, screen_for_show).await
    }

    /// Hides the visible app that hides on focus loss when focus moved elsewhere.
    /// `activated_window_id` is the window the focus event named, if any; when it is the
    /// app's own window no `KWin` query is needed.
    pub async fn handle_focus_change(&self, activated_window_id: Option<String>) -> Result<()> {
        let tracked = {
            let registry = self.registry.lock().await;
            registry.currently_visible_name().and_then(|name| {
                registry
                    .managed_app(name)
                    .filter(|app| app.config.hide_on_focus_lost)
                    .map(|app| (name.to_string(), app.tracked_window_id.clone()))
            })
        };
        let Some((app_name, tracked_id)) = tracked else {
            return Ok(());
        };
        if activated_window_id.is_some() && activated_window_id == tracked_id {
            return Ok(());
        }

        // The event can be queued while another app is hiding, so use KWin's current state
        // rather than the activation that originally triggered the event.
        let active_window_id = self
            .kwin
            .active_window()
            .await?
            .map(|window| window.internal_id);
        if active_window_id != tracked_id {
            let screens = self.current_screens().await;
            self.hide_app(&app_name, &screens).await?;
        }
        Ok(())
    }

    /// Shows `app_name`'s window, or spawns the app when it has none and may be started.
    async fn show_app(
        &self,
        app_name: &str,
        screens: &[ScreenInfo],
        preferred_screen: Option<ScreenInfo>,
    ) -> Result<Option<PendingLaunch>> {
        let registry = self.registry.lock().await;
        let app = require_app(&registry, app_name)?;
        let config = app.config.clone();
        let existing_id = app.tracked_window_id.clone();
        drop(registry);

        if let Some(window) = self.resolve_existing_window(&config, existing_id).await? {
            self.show_window(app_name, &config, window, screens, preferred_screen)
                .await?;
            return Ok(None);
        }
        match config.attach_mode {
            AttachMode::Find => bail!("no existing window matched app '{}'", config.name),
            AttachMode::FindOrStart => {
                spawn::spawn_app(&config)?;
                self.registry.lock().await.mark_launching(app_name);
                info!("started app '{app_name}'; waiting for its window");
                Ok(Some(PendingLaunch {
                    app_name: app_name.to_string(),
                    config,
                }))
            }
        }
    }

    /// Waits for a launched app's window. Runs off the event loop, so it only reads
    /// `KWin` state; [`Self::finish_launch`] acts on the result.
    pub async fn wait_for_launch(&self, launch: PendingLaunch) -> FinishedLaunch {
        let window = self.poll_for_window(&launch.config).await;
        FinishedLaunch {
            app_name: launch.app_name,
            window,
        }
    }

    /// Shows the window a launch found, hiding whichever app became visible meanwhile.
    pub async fn finish_launch(&self, launch: FinishedLaunch) -> Result<()> {
        let FinishedLaunch { app_name, window } = launch;
        let mut registry = self.registry.lock().await;
        registry.mark_launched(&app_name);
        let other_visible = registry
            .currently_visible_name()
            .filter(|name| *name != app_name)
            .map(str::to_string);
        let config = require_app(&registry, &app_name)?.config.clone();
        drop(registry);

        let window = window?
            .with_context(|| format!("spawned app '{app_name}' but no matching window appeared"))?;
        let screens = self.current_screens().await;
        if let Some(other) = other_visible {
            self.hide_app(&other, &screens).await?;
        }
        let internal_id = window.internal_id.clone();
        let placement = self
            .show_window(&app_name, &config, window, &screens, None)
            .await?;
        // A just-launched app can still override the placement while it restores its own
        // geometry. An animated show re-applies it when it ends; an instant show does so once
        // the app had a moment to settle. Toggles wait meanwhile, as they do for an animation.
        if let ShowPlacement::Rects { visible, .. } = placement
            && !config.animation.animates()
        {
            tokio::time::sleep(LAUNCH_SETTLE_DELAY).await;
            // Best effort: the show itself already succeeded.
            if let Err(error) = self.apply_geometry(&internal_id, &visible).await {
                warn!("could not re-apply the placement of '{app_name}': {error:#}");
            }
        }
        Ok(())
    }

    async fn show_window(
        &self,
        app_name: &str,
        config: &AppConfig,
        window: ManagedWindow,
        screens: &[ScreenInfo],
        preferred_screen: Option<ScreenInfo>,
    ) -> Result<ShowPlacement> {
        let placement = self
            .prepare_show(app_name, config, &window, screens, preferred_screen)
            .await?;
        let placed = match &placement {
            ShowPlacement::Rects { visible, hidden } => {
                if let Some(plan) = TransitionPlan::from_config(
                    &config.animation,
                    visible,
                    hidden,
                    TransitionPhase::Show,
                ) {
                    self.run_animation(&window.internal_id, &plan, TransitionPhase::Show)
                        .await
                } else {
                    self.place_and_raise(&window.internal_id, visible).await
                }
            }
            ShowPlacement::KeepFullScreen { screen } => {
                self.raise_full_screen(&window, screens, screen).await
            }
        };

        // Record the show even when placing failed part-way, so the next press hides the
        // window instead of re-running a show the user already saw start.
        // `prepare_show` already tracks the window.
        self.registry.lock().await.mark_visible(app_name);
        placed?;
        info!("showed app '{app_name}'");
        Ok(placement)
    }

    /// Readies `window` for a show and returns how to place it.
    async fn prepare_show(
        &self,
        app_name: &str,
        config: &AppConfig,
        window: &ManagedWindow,
        screens: &[ScreenInfo],
        preferred_screen: Option<ScreenInfo>,
    ) -> Result<ShowPlacement> {
        let registry = self.registry.lock().await;
        let app = require_app(&registry, app_name)?;
        // A window the user left fullscreen when it was hidden stays fullscreen, like F11 in
        // other drop-down apps. Any other maximized or fullscreen state was restored by the
        // app itself, and KWin pins such a window's size, so it would break the placement.
        let keep_full_screen =
            app.tracks(&window.internal_id) && app.hidden_full_screen && window.modes.full_screen;
        drop(registry);
        // Un-minimize any minimized window, e.g. a fullscreen one that hiding minimized.
        if config.hide_behavior == HideBehavior::Minimize || window.minimized {
            self.kwin
                .minimize_window(&window.internal_id, Minimized::No)
                .await?;
        }
        if config.follow_current_desktop {
            self.kwin
                .move_window_to_current_desktop(&window.internal_id)
                .await?;
        }
        let normal_geometry = if keep_full_screen {
            None
        } else {
            self.leave_maximized_and_full_screen(window).await?
        };
        require_app_mut(&mut *self.registry.lock().await, app_name)?
            .take_over(window, normal_geometry);
        if config.hide_decorations && !window.no_border {
            self.kwin
                .set_window_decorations(&window.internal_id, Decorations::Hidden)
                .await?;
        }

        let screen = match preferred_screen {
            Some(screen) => screen,
            None => self.current_screen(config, screens).await?,
        };
        if keep_full_screen {
            return Ok(ShowPlacement::KeepFullScreen { screen });
        }
        screen
            .validate_placement(&config.placement)
            .with_context(|| format!("invalid placement for '{app_name}'"))?;
        let visible = screen.placement_rect(&config.placement);
        let hidden = hidden_rect_for_screens(screens, &visible, config.animation.direction);
        Ok(ShowPlacement::Rects { visible, hidden })
    }

    /// Takes `window` out of maximized and fullscreen state, once the app has acknowledged
    /// it, and returns its normal geometry, if learned. Placing it earlier is undone: the
    /// acknowledgement moves the window back to the geometry it had before it was maximized.
    async fn leave_maximized_and_full_screen(
        &self,
        window: &ManagedWindow,
    ) -> Result<Option<FrameGeometry>> {
        if window.modes.is_normal() {
            return Ok(Some(window.frame_geometry));
        }
        let internal_id = window.internal_id.as_str();
        match self
            .kwin
            .leave_window_modes(internal_id, NORMAL_STATE_TIMEOUT)
            .await?
        {
            ModesLeft::Normal(current) => Ok(Some(current.frame_geometry)),
            // Placing it reports the error.
            ModesLeft::Closed => Ok(None),
            // An app can leave the state without its geometry changing, which the script
            // cannot tell apart from not acknowledging yet.
            ModesLeft::Pending => match self.kwin.window(internal_id).await? {
                Some(current) if current.modes.is_normal() => Ok(Some(current.frame_geometry)),
                _ => {
                    warn!(
                        "window {internal_id} is still maximized or fullscreen; placing it anyway"
                    );
                    Ok(None)
                }
            },
        }
    }

    /// Shows a window kept fullscreen: on `screen`, raised, without an animation.
    async fn raise_full_screen(
        &self,
        window: &ManagedWindow,
        screens: &[ScreenInfo],
        screen: &ScreenInfo,
    ) -> Result<()> {
        if Self::screen_for_window(screens, window).name != screen.name {
            self.kwin
                .send_window_to_screen(&window.internal_id, &screen.name)
                .await?;
        }
        self.kwin
            .bring_window_to_foreground(&window.internal_id)
            .await
    }

    async fn place_and_raise(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()> {
        self.apply_geometry(internal_id, geometry).await?;
        self.kwin.bring_window_to_foreground(internal_id).await
    }

    async fn hide_app(&self, app_name: &str, screens: &[ScreenInfo]) -> Result<()> {
        let registry = self.registry.lock().await;
        let app = require_app(&registry, app_name)?;
        let config = app.config.clone();
        let tracked_window_id = app.tracked_window_id.clone();
        drop(registry);

        let resolved_window = self
            .resolve_existing_window(&config, tracked_window_id)
            .await?;
        if let Some(window) = resolved_window {
            return self.hide_resolved(app_name, &config, window, screens).await;
        }

        warn!(
            "cannot hide app '{}' because no tracked window is attached",
            app_name
        );
        self.registry.lock().await.mark_hidden(app_name);
        Ok(())
    }

    /// Hides `window`, already resolved for `app_name`.
    async fn hide_resolved(
        &self,
        app_name: &str,
        config: &AppConfig,
        window: ManagedWindow,
        screens: &[ScreenInfo],
    ) -> Result<()> {
        // A window found by matching instead of the tracked one, e.g. after the tracked one
        // closed, is taken over first so it does not inherit the old window's restore state.
        let mut registry = self.registry.lock().await;
        let app = require_app_mut(&mut registry, app_name)?;
        if !app.tracks(&window.internal_id) {
            let normal_geometry = window.modes.is_normal().then_some(window.frame_geometry);
            app.take_over(&window, normal_geometry);
            app.hidden_full_screen = false;
        }
        drop(registry);
        let hidden = self.hide_window(config, &window, screens).await;
        let mut registry = self.registry.lock().await;
        if let Some(app) = registry.managed_app_mut(app_name) {
            app.hidden_full_screen = window.modes.full_screen;
        }
        registry.record_window(app_name, window.internal_id);
        // Mark it hidden even when hiding failed part-way, so the next press shows (and fully
        // re-places) the window instead of trying to hide it again.
        registry.mark_hidden(app_name);
        drop(registry);
        hidden?;
        info!("hid app '{app_name}'");
        Ok(())
    }

    async fn hide_window(
        &self,
        config: &AppConfig,
        window: &ManagedWindow,
        screens: &[ScreenInfo],
    ) -> Result<()> {
        // KWin pins a fullscreen window's geometry, so it cannot be moved offscreen.
        if config.hide_behavior == HideBehavior::Minimize || window.modes.full_screen {
            return self
                .kwin
                .minimize_window(&window.internal_id, Minimized::Yes)
                .await;
        }
        // The same holds for a window the user maximized while it was shown; the next show
        // would leave maximized state anyway.
        self.leave_maximized_and_full_screen(window).await?;

        let screen = Self::screen_for_window(screens, window);
        let visible_rect = screen.placement_rect(&config.placement);
        let hidden_rect =
            hidden_rect_for_screens(screens, &visible_rect, config.animation.direction);
        if let Some(plan) = TransitionPlan::from_config(
            &config.animation,
            &visible_rect,
            &hidden_rect,
            TransitionPhase::Hide,
        ) {
            self.run_animation(&window.internal_id, &plan, TransitionPhase::Hide)
                .await
        } else {
            self.apply_hidden_geometry(&window.internal_id, &hidden_rect)
                .await
        }
    }

    async fn run_animation(
        &self,
        internal_id: &str,
        plan: &TransitionPlan,
        phase: TransitionPhase,
    ) -> Result<()> {
        self.apply_window_state(internal_id, plan.setup_state())
            .await?;
        // A window being shown is raised once it's in its start position.
        if phase == TransitionPhase::Show {
            self.kwin.bring_window_to_foreground(internal_id).await?;
        }

        // Pace frames on a fixed schedule so each frame's round trip to KWin overlaps the
        // wait instead of adding to it. The first tick completes immediately.
        let mut ticker = tokio::time::interval(plan.frame_delay());
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        for frame_idx in 1..=plan.frame_count() {
            ticker.tick().await;
            let state = plan.frame_state(frame_idx);
            // The setup state already sized the window and hidden rects keep the visible
            // size, so frames only move it.
            let update = WindowUpdate {
                geometry: state.geometry,
                resize: false,
                opacity: state.opacity,
            };
            if let Err(error) = self.kwin.update_window(internal_id, &update).await {
                // Best effort: don't leave a half-faded window behind after a failed hide.
                if phase == TransitionPhase::Hide
                    && let Err(teardown_error) = self
                        .apply_window_state(internal_id, plan.teardown_state())
                        .await
                {
                    warn!("failed to restore window state after a failed hide: {teardown_error:#}");
                }
                return Err(error);
            }
        }

        self.apply_window_state(internal_id, plan.teardown_state())
            .await
    }

    async fn apply_window_state(&self, internal_id: &str, state: WindowState) -> Result<()> {
        if state.geometry.is_none() && state.opacity.is_none() {
            return Ok(());
        }
        let update = WindowUpdate {
            geometry: state.geometry,
            resize: true,
            opacity: state.opacity,
        };
        self.kwin.update_window(internal_id, &update).await
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

    async fn current_screens(&self) -> Vec<ScreenInfo> {
        match self.kwin.screens().await {
            Ok(screens) if !screens.is_empty() => {
                self.screens.lock().await.clone_from(&screens);
                screens
            }
            Ok(_) => self.screens.lock().await.clone(),
            Err(error) => {
                warn!("failed to refresh screen information; using cached screens: {error:#}");
                self.screens.lock().await.clone()
            }
        }
    }

    /// Returns the screen configured in `placement.screen` when it is connected, otherwise the
    /// screen under the cursor, then the screen of the active window.
    async fn current_screen(
        &self,
        config: &AppConfig,
        screens: &[ScreenInfo],
    ) -> Result<ScreenInfo> {
        if let Some(name) = config.placement.screen.as_deref() {
            if let Some(screen) = screens.iter().find(|screen| screen.name == name) {
                return Ok(screen.clone());
            }
            warn!(
                "screen '{name}' configured for app '{}' was not found; using the screen under the cursor",
                config.name
            );
        }
        // With one screen the cursor and active-window lookups can only pick it.
        if let [only] = screens {
            return Ok(only.clone());
        }

        if let Some(position) = self.kwin.cursor_position().await?
            && let Some(screen) = Self::screen_containing_point(screens, position)
        {
            return Ok(screen.clone());
        }

        if let Some(window) = self.kwin.active_window().await? {
            return Ok(Self::screen_for_geometry(screens, &window.frame_geometry).clone());
        }

        screens
            .first()
            .cloned()
            .context("no screens are known to place the window on")
    }

    fn screen_for_window<'a>(screens: &'a [ScreenInfo], window: &ManagedWindow) -> &'a ScreenInfo {
        Self::screen_for_geometry(screens, &window.frame_geometry)
    }

    fn screen_containing_point(screens: &[ScreenInfo], position: Point) -> Option<&ScreenInfo> {
        screens
            .iter()
            .find(|screen| screen.contains_point(position.x, position.y))
    }

    fn screen_for_geometry<'a>(
        screens: &'a [ScreenInfo],
        geometry: &FrameGeometry,
    ) -> &'a ScreenInfo {
        screens
            .iter()
            .max_by_key(|screen| screen.overlap_area(geometry))
            .expect("ToggleService keeps at least one screen")
    }

    /// Polls the window list every `SPAWN_POLL_INTERVAL` for a window matching `config`
    /// until one appears or `SPAWN_POLL_TIMEOUT` passes.
    async fn poll_for_window(&self, config: &AppConfig) -> Result<Option<ManagedWindow>> {
        let deadline = tokio::time::Instant::now() + SPAWN_POLL_TIMEOUT;
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(SPAWN_POLL_INTERVAL).await;
            if let Some(window) = self.find_matching_window(config).await? {
                return Ok(Some(window));
            }
        }
        Ok(None)
    }

    async fn resolve_existing_window(
        &self,
        config: &AppConfig,
        tracked_window_id: Option<String>,
    ) -> Result<Option<ManagedWindow>> {
        if let Some(window_id) = tracked_window_id
            && let Some(window) = self.kwin.window(&window_id).await?
        {
            return Ok(Some(window));
        }

        if let Some(window) = self.find_matching_window(config).await? {
            return Ok(Some(window));
        }

        Ok(None)
    }

    async fn find_matching_window(&self, config: &AppConfig) -> Result<Option<ManagedWindow>> {
        let windows = self.kwin.window_list().await?;
        let (window, count) = find_best_match(&windows, config);
        if count > 1 {
            warn!(
                "multiple windows matched app '{}'; using the first result",
                config.name
            );
        }
        Ok(window.cloned())
    }

    /// Puts every tracked window back how it was before plasma-drop managed it. A failure
    /// for one app is logged and does not stop the others from being restored.
    pub async fn restore_tracked_windows_on_shutdown(&self) {
        let tracked_windows: Vec<_> = {
            let registry = self.registry.lock().await;
            let visible = registry.currently_visible_name();
            registry
                .managed_apps()
                .filter_map(|app| {
                    // Hiding minimizes a fullscreen window even when the app hides offscreen.
                    let minimized_by_hide = app.config.hide_behavior == HideBehavior::Offscreen
                        && app.hidden_full_screen
                        && visible != Some(app.config.name.as_str());
                    Some((
                        app.config.name.clone(),
                        app.tracked_window_id.clone()?,
                        app.restore,
                        if minimized_by_hide {
                            Minimized::Yes
                        } else {
                            Minimized::No
                        },
                    ))
                })
                .collect()
        };

        for (app_name, window_id, restore, minimized_by_hide) in tracked_windows {
            if let Err(error) = self
                .restore_tracked_window(&app_name, &window_id, restore, minimized_by_hide)
                .await
            {
                warn!("failed to restore '{app_name}': {error:#}");
            }
        }
    }

    async fn restore_tracked_window(
        &self,
        app_name: &str,
        window_id: &str,
        restore: RestoreState,
        minimized_by_hide: Minimized,
    ) -> Result<()> {
        let Some(window) = self.kwin.window(window_id).await? else {
            return Ok(());
        };

        // Only a window plasma-drop minimized itself; one the user minimized stays so.
        if minimized_by_hide == Minimized::Yes && window.minimized {
            self.kwin.minimize_window(window_id, Minimized::No).await?;
        }
        if let Some(RestorePlacement { geometry, modes }) = restore.placement {
            // Without a learned normal geometry only the state is restored.
            if let Some(geometry) = geometry {
                // KWin pins the size of a window the user maximized or made fullscreen.
                self.leave_maximized_and_full_screen(&window).await?;
                self.apply_geometry(window_id, &geometry).await?;
                info!(
                    "restored app '{}' geometry before shutdown (previous geometry was {}x{} at {},{})",
                    app_name, geometry.width, geometry.height, geometry.x, geometry.y
                );
            }
            // After the geometry, which becomes the size the window returns to when it
            // leaves the restored state.
            if !modes.is_normal() {
                self.kwin.set_window_modes(window_id, modes).await?;
                info!("restored app '{app_name}' window state before shutdown ({modes:?})");
            }
        }

        if let Some(no_border) = restore.no_border {
            let decorations = if no_border {
                Decorations::Hidden
            } else {
                Decorations::Shown
            };
            self.kwin
                .set_window_decorations(window_id, decorations)
                .await?;
            info!(
                "restored app '{}' decoration state before shutdown (no_border={})",
                app_name, no_border
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
    // `saturating_duration_since` never panics on reordered instants.
    recent_hotkeys
        .retain(|_, instant| now.saturating_duration_since(*instant) <= HOTKEY_DEBOUNCE_WINDOW);

    match recent_hotkeys.get(shortcut_id) {
        Some(previous) if now.saturating_duration_since(*previous) <= HOTKEY_DEBOUNCE_WINDOW => {
            true
        }
        _ => {
            recent_hotkeys.insert(shortcut_id.to_string(), now);
            false
        }
    }
}

#[cfg(test)]
mod tests;
