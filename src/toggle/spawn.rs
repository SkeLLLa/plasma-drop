use crate::config::AppConfig;
use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::process::Command;
use tracing::info;

/// Spawns `config.command` detached from the daemon and returns the child pid, if known.
pub(super) fn spawn_app(config: &AppConfig) -> Result<Option<u32>> {
    let (program, arguments) = config.command.split_first().with_context(|| {
        format!(
            "app '{}' cannot spawn without filename or command",
            config.name
        )
    })?;

    let mut command = Command::new(program);
    command.args(arguments);
    if let Some(dir) = config.working_directory.as_ref() {
        command.current_dir(dir);
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Detach the child into its own session; the caller also moves it into its own systemd
    // scope so a service restart does not kill it.
    let detach = || {
        // SAFETY: `setsid` takes no arguments and only moves the calling (child)
        // process into a new session.
        if unsafe { libc::setsid() } == -1 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    };
    // SAFETY: `detach` runs in the forked child before `exec`, where only
    // async-signal-safe calls are allowed; it makes one `setsid` call, reads `errno`
    // on failure, and allocates nothing.
    unsafe {
        command.pre_exec(detach);
    }

    let child = command
        .spawn()
        .with_context(|| format!("failed to spawn '{program}'"))?;
    info!("spawned app '{}'", config.name);
    // Dropping `child` does not kill the app (`kill_on_drop` is off); tokio reaps it.
    Ok(child.id())
}
