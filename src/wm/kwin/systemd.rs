//! Moves spawned apps into their own transient systemd user scopes, the way
//! `systemd-run --user --scope` and KDE's `KProcessRunner` do, so stopping or restarting the
//! plasma-drop service does not take the user's apps down with it.

use anyhow::{Context, Result};
use zbus::Connection;
use zbus::Variant;

const SYSTEMD_SERVICE: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const SYSTEMD_MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";

/// Transient scope unit name for `app_name`'s process `pid`.
///
/// Unit names only allow `[A-Za-z0-9:_.\-]`; everything except ASCII alphanumerics, `_` and
/// `.` is mapped to `_` so the `app-plasma_drop-<name>-<pid>` layout stays unambiguous.
pub(super) fn scope_unit_name(app_name: &str, pid: u32) -> String {
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
    if sanitized.is_empty() {
        sanitized.push_str("app");
    }
    format!("app-plasma_drop-{sanitized}-{pid}.scope")
}

/// Asks the systemd user manager to start a transient scope holding `pid`, moving the process
/// out of the daemon's cgroup.
pub(super) async fn move_into_scope(
    connection: &Connection,
    app_name: &str,
    pid: u32,
) -> Result<()> {
    let name = scope_unit_name(app_name, pid);
    let properties: Vec<(&str, Variant<'_>)> = vec![
        ("PIDs", Variant::from(vec![pid])),
        (
            "Description",
            Variant::from(format!("PlasmaDrop app {app_name}")),
        ),
        ("CollectMode", Variant::from("inactive-or-failed")),
    ];
    let auxiliary: Vec<(&str, Vec<(&str, Variant<'_>)>)> = Vec::new();
    connection
        .call_method(
            Some(SYSTEMD_SERVICE),
            SYSTEMD_PATH,
            Some(SYSTEMD_MANAGER_INTERFACE),
            "StartTransientUnit",
            &(name.as_str(), "fail", properties, auxiliary),
        )
        .await
        .with_context(|| {
            format!("failed to move '{app_name}' (pid {pid}) into its own systemd scope")
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::scope_unit_name;

    #[test]
    fn separators_and_spaces_are_mapped() {
        assert_eq!(
            scope_unit_name("my term/dev-1", 7),
            "app-plasma_drop-my_term_dev_1-7.scope"
        );
    }

    #[test]
    fn empty_name_falls_back_to_app() {
        assert_eq!(scope_unit_name("", 9), "app-plasma_drop-app-9.scope");
    }
}
