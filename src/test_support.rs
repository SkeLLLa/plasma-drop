//! Helpers shared by unit tests.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A fresh temporary directory, removed on drop even when an assertion fails.
pub struct TempDir(PathBuf);

impl TempDir {
    /// Creates an empty directory unique to `name` and this test process.
    pub fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("plasma-drop-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Creates `relative`, and its parent directories, as an empty file with `mode`.
    pub fn file(&self, relative: &str, mode: u32) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
