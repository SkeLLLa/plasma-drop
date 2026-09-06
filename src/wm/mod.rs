pub mod kwin;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;

pub use types::{FrameGeometry, ManagedWindow, Point, find_best_match};

pub const HOTKEY_PREFIX: &str = "plasma_drop_hotkey_";

#[async_trait]
pub trait WindowManager: Send + Sync {
    async fn get_window_list(&self) -> Result<Vec<ManagedWindow>>;
    async fn get_window(&self, internal_id: &str) -> Result<Option<ManagedWindow>>;
    async fn get_active_window(&self) -> Result<Option<ManagedWindow>>;
    async fn get_cursor_position(&self) -> Result<Option<Point>>;
    async fn get_support_information_text(&self) -> Result<Option<String>> {
        Ok(None)
    }
    async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()>;
    async fn resize_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()>;
    async fn set_window_opacity(&self, internal_id: &str, opacity: f64) -> Result<()>;
    async fn set_window_no_border(&self, internal_id: &str, no_border: bool) -> Result<()>;
    async fn minimize_window(&self, internal_id: &str, minimized: bool) -> Result<()>;
    async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()>;
    async fn move_window_to_current_desktop(&self, internal_id: &str) -> Result<()>;
}
