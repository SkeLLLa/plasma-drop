use zbus::proxy;

#[proxy(
    interface = "org.kde.KWin",
    default_service = "org.kde.KWin",
    default_path = "/KWin"
)]
pub trait KWin {
    #[zbus(name = "supportInformation")]
    fn support_information(&self) -> zbus::Result<String>;
}

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
}

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
