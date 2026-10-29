//! Runs apps in their own transient systemd user scopes, the way desktop launchers do, so
//! stopping or restarting the plasma-drop service cannot take an app, or anything it forks,
//! down with it.
//!
//! The daemon starts each app through the hidden `launch-scoped` subcommand: that process
//! moves itself into a new scope and then becomes the app. A scope that cannot be created
//! is reported on stderr, and the app starts anyway, unscoped.

use anyhow::{Context, Result};
use std::convert::Infallible;
use std::ffi::{OsStr, OsString};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use zbus::Connection;
use zbus::Variant;

/// The subcommand of the daemon's own binary that starts an app in a scope.
pub const LAUNCH_SCOPED: &str = "launch-scoped";

const SYSTEMD_SERVICE: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const SYSTEMD_MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
/// How long creating the scope may take in all, before the app starts without it.
const SCOPE_TIMEOUT: Duration = Duration::from_secs(2);
const SCOPE_JOIN_POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Longest app name kept in a scope unit name, within systemd's 255-byte limit.
const MAX_SCOPE_APP_NAME_LEN: usize = 128;

/// Whether a systemd user manager is running: it accepts connections on its private socket
/// in `runtime_dir`. A socket file left behind by a manager that is gone refuses them.
/// Distros without systemd (elogind, `OpenRC`, runit) have no such socket.
pub fn user_manager_running(runtime_dir: Option<&OsStr>) -> bool {
    runtime_dir
        .is_some_and(|dir| UnixStream::connect(Path::new(dir).join("systemd/private")).is_ok())
}

/// Transient scope unit name for `app_name`, following the `app-<launcher>-<app>-<random>`
/// layout that desktop launchers use.
///
/// Unit names only allow `[A-Za-z0-9:_.\-]`; everything except ASCII alphanumerics, `_` and
/// `.` is mapped to `_` so the `app-plasma_drop-<name>-<suffix>` layout stays unambiguous, and
/// the name is shortened to leave room for the rest within systemd's length limit.
pub fn scope_unit_name(app_name: &str, suffix: &str) -> String {
    let mut sanitized: String = app_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    // ASCII only, so this cuts on a char boundary.
    sanitized.truncate(MAX_SCOPE_APP_NAME_LEN);
    if sanitized.is_empty() {
        sanitized.push_str("app");
    }
    format!("app-plasma_drop-{sanitized}-{suffix}.scope")
}

/// Moves this process into the new scope `unit`, then replaces it with `command`, whose
/// stdio goes to `/dev/null`. Returns only when the command could not be executed.
pub async fn launch_scoped(
    unit: &str,
    description: &str,
    command: &[OsString],
) -> Result<Infallible> {
    let (program, arguments) = command.split_first().context("no command to launch")?;
    let entered = tokio::time::timeout(SCOPE_TIMEOUT, enter_scope(unit, description))
        .await
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "systemd did not set up scope {unit} in time"
            ))
        });
    if let Err(error) = entered {
        // stderr is still the daemon's, so this reaches its journal.
        eprintln!("plasma-drop: starting the app outside a systemd scope: {error:#}");
    }
    let error = std::process::Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .exec();
    Err(error).with_context(|| format!("failed to start '{}'", program.to_string_lossy()))
}

/// Asks the user manager for a transient scope holding this process and waits until the
/// process is in it.
async fn enter_scope(unit: &str, description: &str) -> Result<()> {
    let connection = Connection::session()
        .await
        .context("cannot connect to the session bus")?;
    let properties: Vec<(&str, Variant<'_>)> = vec![
        ("PIDs", Variant::from(vec![std::process::id()])),
        ("Description", Variant::from(description)),
        ("CollectMode", Variant::from("inactive-or-failed")),
    ];
    let auxiliary: Vec<(&str, Vec<(&str, Variant<'_>)>)> = Vec::new();
    connection
        .call_method(
            Some(SYSTEMD_SERVICE),
            SYSTEMD_PATH,
            Some(SYSTEMD_MANAGER_INTERFACE),
            "StartTransientUnit",
            &(unit, "fail", properties, auxiliary),
        )
        .await
        .with_context(|| format!("systemd refused scope {unit}"))?;

    // The manager moves the process when it runs the start job, after replying.
    loop {
        let cgroup = tokio::fs::read_to_string("/proc/self/cgroup")
            .await
            .context("cannot read /proc/self/cgroup")?;
        if in_scope(&cgroup, unit) {
            return Ok(());
        }
        tokio::time::sleep(SCOPE_JOIN_POLL_INTERVAL).await;
    }
}

/// Whether `cgroup`, the contents of `/proc/<pid>/cgroup`, places the process in `unit`.
fn in_scope(cgroup: &str, unit: &str) -> bool {
    cgroup
        .lines()
        .filter_map(|line| line.rsplit_once('/'))
        .any(|(_, leaf)| leaf == unit)
}

#[cfg(test)]
mod tests {
    use super::{in_scope, scope_unit_name, user_manager_running};
    use crate::test_support::TempDir;
    use std::os::unix::net::UnixListener;

    #[test]
    fn user_manager_runs_when_its_socket_accepts_connections() {
        let dir = TempDir::new("running");
        std::fs::create_dir(dir.path().join("systemd")).unwrap();
        let _manager = UnixListener::bind(dir.path().join("systemd/private")).unwrap();
        assert!(user_manager_running(Some(dir.path().as_os_str())));
    }

    #[test]
    fn no_user_manager_without_a_socket() {
        let dir = TempDir::new("elogind");
        assert!(!user_manager_running(Some(dir.path().as_os_str())));
        assert!(!user_manager_running(None));
    }

    #[test]
    fn no_user_manager_when_the_socket_is_stale() {
        let dir = TempDir::new("stale");
        dir.file("systemd/private", 0o600);
        assert!(!user_manager_running(Some(dir.path().as_os_str())));
    }

    #[test]
    fn cgroup_places_the_process_in_its_scope() {
        let unit = "app-plasma_drop-calc-1a.scope";
        let scoped = format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{unit}\n");
        let service =
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/plasma-drop.service\n";
        assert!(in_scope(&scoped, unit));
        assert!(!in_scope(service, unit));
        assert!(!in_scope(&format!("0::/x/{unit}.extra\n"), unit));
    }

    #[test]
    fn separators_and_spaces_are_mapped() {
        assert_eq!(
            scope_unit_name("my term/dev-1", "7"),
            "app-plasma_drop-my_term_dev_1-7.scope"
        );
    }

    #[test]
    fn empty_name_falls_back_to_app() {
        assert_eq!(scope_unit_name("", "9"), "app-plasma_drop-app-9.scope");
    }

    #[test]
    fn long_names_stay_within_the_unit_name_limit() {
        let name = scope_unit_name(&"x".repeat(400), &format!("{:x}", u128::MAX));
        assert!(name.len() <= 255, "{} bytes", name.len());
    }
}
