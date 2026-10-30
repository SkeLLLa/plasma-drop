use zbus::proxy;

#[proxy(
    interface = "org.kde.kwin.Scripting",
    default_service = "org.kde.KWin",
    default_path = "/Scripting"
)]
pub trait KWinScripting {
    #[zbus(name = "start")]
    fn start(&self) -> zbus::Result<()>;

    #[zbus(name = "loadScript")]
    fn load_script(&self, file_path: &str, plugin_name: &str) -> zbus::Result<i32>;

    #[zbus(name = "isScriptLoaded")]
    fn is_script_loaded(&self, plugin_name: &str) -> zbus::Result<bool>;

    #[zbus(name = "unloadScript")]
    fn unload_script(&self, plugin_name: &str) -> zbus::Result<bool>;
}

#[proxy(
    interface = "org.kde.KGlobalAccel",
    default_service = "org.kde.kglobalaccel",
    default_path = "/kglobalaccel"
)]
pub trait KGlobalAccel {
    #[zbus(name = "unregister")]
    fn unregister(&self, component_unique: &str, shortcut_unique: &str) -> zbus::Result<bool>;

    /// The keys stored for `[component, action, component friendly, action friendly]`.
    #[zbus(name = "shortcutKeys")]
    fn shortcut_keys(&self, action_id: &[&str]) -> zbus::Result<Vec<(Vec<i32>,)>>;

    /// Every shortcut of every component bound to `key`; search type 0 matches it exactly.
    #[zbus(name = "globalShortcutsByKey")]
    fn global_shortcuts_by_key(
        &self,
        key: &(Vec<i32>,),
        search_type: &(i32,),
    ) -> zbus::Result<Vec<ShortcutInfo>>;
}

/// `KGlobalShortcutInfo` on the wire: action, action friendly name, component, component
/// friendly name, context, context friendly name, keys, default keys.
pub type ShortcutInfo = (
    String,
    String,
    String,
    String,
    String,
    String,
    Vec<i32>,
    Vec<i32>,
);

#[proxy(
    interface = "org.kde.kglobalaccel.Component",
    default_service = "org.kde.kglobalaccel",
    default_path = "/component/kwin"
)]
pub trait KGlobalAccelComponent {
    #[zbus(name = "shortcutNames")]
    fn shortcut_names(&self) -> zbus::Result<Vec<String>>;

    #[zbus(name = "cleanUp")]
    fn clean_up(&self) -> zbus::Result<bool>;
}
