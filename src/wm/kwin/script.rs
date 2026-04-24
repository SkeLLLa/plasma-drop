use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::fs;

pub const SCRIPT_FILE_NAME: &str = "plasma-drop.kwin.js";
const SCRIPT_SOURCE: &str = include_str!("../../../resources/plasma-drop.kwin.js");

pub async fn ensure_script_file() -> Result<PathBuf> {
    let path = std::env::temp_dir().join(SCRIPT_FILE_NAME);
    fs::write(&path, SCRIPT_SOURCE).await.with_context(|| {
        format!(
            "failed to write embedded KWin script to '{}'",
            path.display()
        )
    })?;
    Ok(path)
}
