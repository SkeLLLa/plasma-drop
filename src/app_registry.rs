use crate::config::AppConfig;
use crate::wm::FrameGeometry;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ManagedApp {
    pub config: AppConfig,
    pub tracked_window_id: Option<String>,
    pub restore_geometry: Option<FrameGeometry>,
    pub restore_no_border: Option<bool>,
    pub visible: bool,
    pub shortcut_id: String,
}

#[derive(Debug)]
pub struct AppRegistry {
    apps: HashMap<String, ManagedApp>,
    shortcuts: HashMap<String, String>,
    visible_app: Option<String>,
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

    pub fn set_visible(&mut self, name: &str, visible: bool) {
        if let Some(app) = self.apps.get_mut(name) {
            app.visible = visible;
        }
        if visible {
            self.visible_app = Some(name.to_string());
        } else if self.visible_app.as_deref() == Some(name) {
            self.visible_app = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppRegistry, ManagedApp};
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
            restore_geometry: None,
            restore_no_border: None,
            visible: false,
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
    fn set_visible_true_marks_the_app_and_becomes_the_visible_one() {
        let mut registry = registry();
        registry.set_visible("kitty", true);

        assert!(registry.managed_app("kitty").unwrap().visible);
        assert_eq!(registry.currently_visible_name(), Some("kitty"));
    }

    #[test]
    fn set_visible_false_clears_the_visible_app_when_it_matches() {
        let mut registry = registry();
        registry.set_visible("kitty", true);
        registry.set_visible("kitty", false);

        assert!(!registry.managed_app("kitty").unwrap().visible);
        assert_eq!(registry.currently_visible_name(), None);
    }

    #[test]
    fn hiding_an_app_that_is_not_the_visible_one_does_not_clear_the_tracked_app() {
        // Regression guard: `set_visible(x, false)` must only clear
        // `currently_visible_name()` when `x` is the app it currently points
        // to, otherwise a stray hide of a background app would drop the
        // pointer to the app that is actually still shown.
        let mut registry = registry();
        registry.set_visible("kitty", true);
        registry.set_visible("dolphin", false);

        assert_eq!(registry.currently_visible_name(), Some("kitty"));
    }
}
