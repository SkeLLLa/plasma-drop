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
