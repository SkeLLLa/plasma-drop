use crate::config::AppConfig;
use crate::wm::{FrameGeometry, ManagedWindow, WindowModes};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct ManagedApp {
    pub config: AppConfig,
    pub tracked_window_id: Option<String>,
    pub restore: RestoreState,
    /// Whether the window was fullscreen when plasma-drop last hid it.
    pub hidden_full_screen: bool,
    pub shortcut_id: String,
}

/// How a tracked window looked before plasma-drop took it over; put back on shutdown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestoreState {
    pub placement: Option<RestorePlacement>,
    pub no_border: Option<bool>,
}

/// Where a window was and whether it was maximized or fullscreen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestorePlacement {
    /// The geometry outside maximized and fullscreen state; `None` when the app never
    /// confirmed leaving that state, so it was not learned.
    pub geometry: Option<FrameGeometry>,
    pub modes: WindowModes,
}

impl ManagedApp {
    /// Whether `internal_id` is the window tracked for this app.
    pub fn tracks(&self, internal_id: &str) -> bool {
        self.tracked_window_id.as_deref() == Some(internal_id)
    }

    /// Tracks `window`, first recording how it looked before plasma-drop changes it, unless
    /// that is already known for this window. `normal_geometry` is its geometry outside
    /// maximized and fullscreen state, when known. Tracking it right away keeps a show that
    /// fails part-way from recording the already-changed window as the original next time.
    pub fn take_over(&mut self, window: &ManagedWindow, normal_geometry: Option<FrameGeometry>) {
        let is_new_window = !self.tracks(&window.internal_id);
        if is_new_window || self.restore.placement.is_none() {
            self.restore.placement = Some(RestorePlacement {
                geometry: normal_geometry,
                modes: window.modes,
            });
        }
        if self.config.hide_decorations && (is_new_window || self.restore.no_border.is_none()) {
            self.restore.no_border = Some(window.no_border);
        }
        self.tracked_window_id = Some(window.internal_id.clone());
    }
}

#[derive(Debug)]
pub struct AppRegistry {
    apps: HashMap<String, ManagedApp>,
    shortcuts: HashMap<String, String>,
    visible_app: Option<String>,
    /// Apps whose command was spawned and whose window has not appeared yet.
    launching: HashSet<String>,
}

impl AppRegistry {
    pub fn new(apps: Vec<ManagedApp>) -> Self {
        let mut app_map = HashMap::new();
        let mut shortcut_map = HashMap::new();
        for app in apps {
            shortcut_map.insert(app.shortcut_id.clone(), app.config.name.clone());
            app_map.insert(app.config.name.clone(), app);
        }

        Self {
            apps: app_map,
            shortcuts: shortcut_map,
            visible_app: None,
            launching: HashSet::new(),
        }
    }

    pub fn app_for_shortcut(&self, shortcut_id: &str) -> Option<&str> {
        self.shortcuts.get(shortcut_id).map(String::as_str)
    }

    pub fn managed_app(&self, name: &str) -> Option<&ManagedApp> {
        self.apps.get(name)
    }

    pub fn managed_app_mut(&mut self, name: &str) -> Option<&mut ManagedApp> {
        self.apps.get_mut(name)
    }

    pub fn managed_apps(&self) -> impl Iterator<Item = &ManagedApp> {
        self.apps.values()
    }

    pub fn currently_visible_name(&self) -> Option<&str> {
        self.visible_app.as_deref()
    }

    /// Records `window_id` as the window tracked for `name`.
    pub fn record_window(&mut self, name: &str, window_id: String) {
        if let Some(app) = self.apps.get_mut(name) {
            app.tracked_window_id = Some(window_id);
        }
    }

    /// Forgets the window tracked for `name`, e.g. after it was closed outside plasma-drop.
    pub fn forget_window(&mut self, name: &str) {
        if let Some(app) = self.apps.get_mut(name) {
            app.tracked_window_id = None;
        }
    }

    /// Marks `name` shown; it becomes the currently visible app.
    pub fn mark_visible(&mut self, name: &str) {
        self.visible_app = Some(name.to_string());
    }

    /// Marks `name` hidden, clearing the currently visible app only if it was `name`.
    pub fn mark_hidden(&mut self, name: &str) {
        if self.visible_app.as_deref() == Some(name) {
            self.visible_app = None;
        }
    }

    /// Records that `name` was spawned and its window is awaited.
    pub fn mark_launching(&mut self, name: &str) {
        self.launching.insert(name.to_string());
    }

    /// Records that waiting for `name`'s window ended, whether or not one appeared.
    pub fn mark_launched(&mut self, name: &str) {
        self.launching.remove(name);
    }

    pub fn is_launching(&self, name: &str) -> bool {
        self.launching.contains(name)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppRegistry, ManagedApp, RestoreState};
    use crate::config::{AnimationConfig, AppConfig, AttachMode, HideBehavior, PlacementConfig};
    use crate::hotkey::Hotkey;

    fn managed_app(name: &str, shortcut_id: &str) -> ManagedApp {
        ManagedApp {
            config: AppConfig {
                name: name.into(),
                hotkey: Hotkey::parse("ctrl+grave").unwrap(),
                filename: Some(name.into()),
                command: vec![name.into()],
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
            },
            tracked_window_id: None,
            restore: RestoreState::default(),
            hidden_full_screen: false,
            shortcut_id: shortcut_id.into(),
        }
    }

    fn registry() -> AppRegistry {
        AppRegistry::new(vec![
            managed_app("kitty", "shortcut-terminal"),
            managed_app("dolphin", "shortcut-files"),
        ])
    }

    #[test]
    fn apps_are_indexed_by_name_and_by_shortcut() {
        let registry = registry();
        assert_eq!(
            registry.app_for_shortcut("shortcut-terminal"),
            Some("kitty")
        );
        assert_eq!(registry.app_for_shortcut("shortcut-files"), Some("dolphin"));
        assert_eq!(registry.app_for_shortcut("unknown-shortcut"), None);
        assert_eq!(registry.managed_apps().count(), 2);
    }

    #[test]
    fn managed_app_looks_up_by_name_and_reports_missing() {
        let registry = registry();
        assert_eq!(
            registry
                .managed_app("kitty")
                .map(|app| app.shortcut_id.as_str()),
            Some("shortcut-terminal")
        );
        assert!(registry.managed_app("konsole").is_none());
    }

    #[test]
    fn managed_app_mut_allows_in_place_updates() {
        let mut registry = registry();
        registry.managed_app_mut("kitty").unwrap().tracked_window_id = Some("{abc}".into());
        assert_eq!(
            registry
                .managed_app("kitty")
                .unwrap()
                .tracked_window_id
                .as_deref(),
            Some("{abc}")
        );
    }

    #[test]
    fn mark_visible_marks_the_app_and_becomes_the_visible_one() {
        let mut registry = registry();
        registry.mark_visible("kitty");

        assert_eq!(registry.currently_visible_name(), Some("kitty"));
    }

    #[test]
    fn mark_hidden_clears_the_visible_app_when_it_matches() {
        let mut registry = registry();
        registry.mark_visible("kitty");
        registry.mark_hidden("kitty");

        assert_eq!(registry.currently_visible_name(), None);
    }

    #[test]
    fn hiding_an_app_that_is_not_the_visible_one_does_not_clear_the_tracked_app() {
        // Regression guard: `mark_hidden(x)` must only clear
        // `currently_visible_name()` when `x` is the app it currently points
        // to, otherwise a stray hide of a background app would drop the
        // pointer to the app that is actually still shown.
        let mut registry = registry();
        registry.mark_visible("kitty");
        registry.mark_hidden("dolphin");

        assert_eq!(registry.currently_visible_name(), Some("kitty"));
    }
}
