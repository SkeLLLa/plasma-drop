pub mod kwin;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;

use crate::screen::ScreenInfo;
use std::time::Duration;

pub use types::{
    Decorations, FrameGeometry, ManagedWindow, Minimized, ModesLeft, Point, WindowModes,
    WindowUpdate, find_best_match,
};

pub const HOTKEY_PREFIX: &str = "plasma_drop_hotkey_";

#[async_trait]
pub trait WindowManager: Send + Sync {
    async fn window_list(&self) -> Result<Vec<ManagedWindow>>;
    async fn window(&self, internal_id: &str) -> Result<Option<ManagedWindow>>;
    async fn active_window(&self) -> Result<Option<ManagedWindow>>;
    async fn cursor_position(&self) -> Result<Option<Point>>;
    /// Fresh screen layout; empty when the window manager cannot report one.
    async fn screens(&self) -> Result<Vec<ScreenInfo>>;
    async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()>;
    async fn resize_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()>;
    async fn set_window_decorations(
        &self,
        internal_id: &str,
        decorations: Decorations,
    ) -> Result<()>;
    async fn minimize_window(&self, internal_id: &str, minimized: Minimized) -> Result<()>;
    /// Maximizes or leaves maximized state and enters or leaves fullscreen, per `modes`.
    async fn set_window_modes(&self, internal_id: &str, modes: WindowModes) -> Result<()>;
    /// Takes the window out of maximized and fullscreen state and waits up to `timeout` for
    /// the app to acknowledge that, which on Wayland also moves the window to its normal
    /// geometry.
    async fn leave_window_modes(&self, internal_id: &str, timeout: Duration) -> Result<ModesLeft>;
    /// Moves the window to the screen named `screen`.
    async fn send_window_to_screen(&self, internal_id: &str, screen: &str) -> Result<()>;
    async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()>;
    async fn move_window_to_current_desktop(&self, internal_id: &str) -> Result<()>;
    /// Keeps a window hidden offscreen at `position`, moving it back whenever the window
    /// manager pulls it onto a screen, until [`Self::unpark_window`].
    async fn park_window(&self, internal_id: &str, position: Point) -> Result<()>;
    /// Keeps a shown window at `geometry`, moving it back whenever the window manager
    /// re-places it, until the user moves it, or [`Self::unpark_window`], a park or any
    /// other geometry change from the daemon.
    async fn pin_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()>;
    /// Stops keeping the window where [`Self::park_window`] or [`Self::pin_window`] put it;
    /// a no-op when it is not held.
    async fn unpark_window(&self, internal_id: &str) -> Result<()>;
    /// Moves focus off a hidden window that still has it, to the window the user most
    /// recently used on `screen`, the screen it was shown on. Returns a description of the
    /// newly focused window, `None` when focus did not move to a window.
    async fn release_focus(&self, internal_id: &str, screen: &str) -> Result<Option<String>>;
    /// Applies `update` as a move, then a resize (when `update.resize`), then an opacity
    /// change.
    async fn update_window(&self, internal_id: &str, update: &WindowUpdate) -> Result<()>;
}
