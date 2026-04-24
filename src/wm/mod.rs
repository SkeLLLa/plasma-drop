pub mod kwin;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;

pub use types::{find_best_match, FrameGeometry, ManagedWindow};

pub const HOTKEY_PREFIX: &str = "plasma_drop_hotkey_";

#[async_trait]
pub trait WindowManager: Send + Sync {
    async fn get_window_list(&self) -> Result<Vec<ManagedWindow>>;
    async fn get_window(&self, internal_id: &str) -> Result<Option<ManagedWindow>>;
    async fn move_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()>;
    async fn resize_window(&self, internal_id: &str, geometry: &FrameGeometry) -> Result<()>;
    async fn set_window_opacity(&self, internal_id: &str, opacity: f64) -> Result<()>;
    async fn bring_window_to_foreground(&self, internal_id: &str) -> Result<()>;
}
