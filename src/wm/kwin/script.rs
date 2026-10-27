use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::fs;

pub const SCRIPT_FILE_NAME: &str = "plasma-drop.kwin.js";
const SCRIPT_SOURCE: &str = include_str!("../../../resources/plasma-drop.kwin.js");

/// Writes the embedded `KWin` script into `$XDG_RUNTIME_DIR` (falling back to the system
/// temp directory) and returns its path.
///
/// The file is written under a temporary name and renamed over the target, so a symlink
/// planted at the target path is replaced rather than followed.
pub async fn ensure_script_file() -> Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let path = dir.join(SCRIPT_FILE_NAME);
    let tmp_path = dir.join(format!("{SCRIPT_FILE_NAME}.tmp-{}", std::process::id()));
    fs::write(&tmp_path, SCRIPT_SOURCE).await.with_context(|| {
        format!(
            "failed to write embedded KWin script to '{}'",
            tmp_path.display()
        )
    })?;
    if let Err(error) = fs::rename(&tmp_path, &path).await {
        // Best effort: the rename error is what matters; a leftover temp file is harmless.
        let _ = fs::remove_file(&tmp_path).await;
        return Err(error)
            .with_context(|| format!("failed to move KWin script into '{}'", path.display()));
    }
    Ok(path)
}
