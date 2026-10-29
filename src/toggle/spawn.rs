use super::SPAWN_POLL_TIMEOUT;
use crate::config::AppConfig;
use crate::scope::{LAUNCH_SCOPED, scope_unit_name, user_manager_running};
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::process::{Child, Command};
use tracing::{debug, info, warn};

/// The daemon's own binary, which starts apps in scopes. It still works after a package
/// upgrade replaced the file.
const SELF_EXE: &str = "/proc/self/exe";

/// Spawns `config.command` detached from the daemon.
///
/// The app runs in its own transient systemd user scope (see [`crate::scope`]) when a user
/// manager is running, so stopping or restarting the plasma-drop service cannot take the app
/// or anything it forks down with it. Otherwise it is started unscoped.
pub(super) fn spawn_app(config: &AppConfig) -> Result<()> {
    let (program, _) = config.command.split_first().with_context(|| {
        format!(
            "app '{}' cannot spawn without filename or command",
            config.name
        )
    })?;
    // Check the program up front: the scope launcher starts even when the app does not
    // exist, and its failure would only show as a window that never appears. The app is
    // still started by its configured name, so its argv[0] stays as before.
    resolve_program(
        program,
        config.working_directory.as_deref(),
        std::env::var_os("PATH").as_deref(),
    )
    .with_context(|| format!("failed to spawn app '{}'", config.name))?;
    let program = Path::new(program);

    let scope_unit = scopes_available().then(|| scope_unit_name(&config.name, &unique_suffix()));
    // Without systemd nothing kills the detached app when the daemon stops.
    if scope_unit.is_none() && runs_as_systemd_service() {
        warn!(
            "cannot start '{}' in its own systemd scope; it stops with the plasma-drop service",
            config.name
        );
    }
    let spawned = spawn_detached(build_command(config, program, scope_unit.as_deref()));
    let mut child = match spawned {
        Ok(child) => child,
        // The launcher itself could not be executed, so nothing started yet.
        Err(error) if scope_unit.is_some() => {
            warn!(
                "cannot start '{}' through the scope launcher ({error}); trying without it",
                config.name
            );
            spawn_detached(build_command(config, program, None))
                .with_context(|| format!("failed to spawn '{}'", program.display()))?
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to spawn '{}'", program.display()));
        }
    };
    info!("spawned app '{}'", config.name);
    // Reap the child and report a failed start, which would otherwise only surface as a
    // window that never appears. The scope launcher becomes the app, so this is the app's
    // own status. A failure counts as a failed start while the daemon is still waiting for
    // the app's window. The task is detached on purpose and ends when the app exits.
    let app_name = config.name.clone();
    let started = Instant::now();
    tokio::spawn(async move {
        match child.wait().await {
            Ok(status) if !status.success() && started.elapsed() < SPAWN_POLL_TIMEOUT => {
                warn!("app '{app_name}' exited with {status} while starting");
            }
            Ok(status) => debug!("app '{app_name}' exited with {status}"),
            Err(error) => warn!("failed to wait for app '{app_name}': {error}"),
        }
    });
    Ok(())
}

/// Spawns `command` in its own session, away from the daemon's terminal and signals.
fn spawn_detached(mut command: Command) -> std::io::Result<Child> {
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
    command.spawn()
}

/// The command that starts `config`'s app from the resolved `program`, through the scope
/// launcher when `scope_unit` is given.
fn build_command(config: &AppConfig, program: &Path, scope_unit: Option<&str>) -> Command {
    let mut command = scope_unit.map_or_else(
        || Command::new(program),
        |unit| {
            let mut command = Command::new(SELF_EXE);
            command
                .args(launcher_arguments(&config.name, unit))
                .arg(program);
            command
        },
    );
    command.args(config.command.iter().skip(1));
    if let Some(dir) = config.working_directory.as_ref() {
        command.current_dir(dir);
    }
    // The launcher reports a scope it could not create on the daemon's stderr, then sends
    // the app's own output to /dev/null.
    let stderr = if scope_unit.is_some() {
        Stdio::inherit()
    } else {
        Stdio::null()
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    command
}

/// Arguments of the scope launcher for `app_name` in `unit`; the app's command follows.
fn launcher_arguments(app_name: &str, unit: &str) -> Vec<String> {
    vec![
        LAUNCH_SCOPED.to_owned(),
        format!("--unit={unit}"),
        format!("--description=PlasmaDrop app {app_name}"),
        "--".to_owned(),
    ]
}

/// Finds `program` the way `exec` would: a name containing `/` is a path (relative to
/// `working_directory`, when set), any other name is looked up in `path`.
fn resolve_program(
    program: &str,
    working_directory: Option<&Path>,
    path: Option<&OsStr>,
) -> Result<PathBuf> {
    if program.contains('/') {
        let candidate =
            working_directory.map_or_else(|| PathBuf::from(program), |dir| dir.join(program));
        if is_executable(&candidate) {
            return Ok(candidate);
        }
        bail!("'{program}' is not an executable file");
    }
    path.into_iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
        .with_context(|| format!("'{program}' was not found on PATH"))
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Whether apps can be put into scopes. Unit tests always start them unscoped: the toggle
/// tests really spawn `true`, and must not leave units behind.
fn scopes_available() -> bool {
    !cfg!(test) && user_manager_running(std::env::var_os("XDG_RUNTIME_DIR").as_deref())
}

/// Whether the daemon runs as a systemd service, whose cgroup is killed when it stops.
fn runs_as_systemd_service() -> bool {
    std::env::var_os("INVOCATION_ID").is_some()
}

/// A suffix that keeps scope names unique across launches and daemon restarts.
fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::{SELF_EXE, build_command, launcher_arguments, resolve_program};
    use crate::config::{AnimationConfig, AppConfig, AttachMode, HideBehavior, PlacementConfig};
    use crate::hotkey::Hotkey;
    use crate::test_support::TempDir;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

    fn app(command: &[&str]) -> AppConfig {
        AppConfig {
            name: "my term".into(),
            hotkey: Hotkey::parse("super+f12").unwrap(),
            filename: None,
            command: command.iter().map(|arg| (*arg).to_owned()).collect(),
            process_name: None,
            window_title: None,
            attach_mode: AttachMode::FindOrStart,
            working_directory: Some(PathBuf::from("/srv/work")),
            hide_decorations: false,
            hide_behavior: HideBehavior::Offscreen,
            hide_on_focus_lost: false,
            follow_current_desktop: false,
            placement: PlacementConfig::default(),
            animation: AnimationConfig::default(),
        }
    }

    fn argv(command: &tokio::process::Command) -> Vec<String> {
        let command = command.as_std();
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn scoped_command_runs_the_app_through_the_launcher() {
        let unit = "app-plasma_drop-my_term-1a.scope";
        let command = build_command(
            &app(&["wezterm", "start", "--cwd", "/tmp"]),
            Path::new("/usr/bin/wezterm"),
            Some(unit),
        );
        let mut expected = vec![SELF_EXE.to_owned()];
        expected.extend(launcher_arguments("my term", unit));
        expected.extend(["/usr/bin/wezterm", "start", "--cwd", "/tmp"].map(String::from));
        assert_eq!(argv(&command), expected);
        assert_eq!(
            command.as_std().get_current_dir(),
            Some(Path::new("/srv/work"))
        );
    }

    #[test]
    fn unscoped_command_runs_the_app_directly() {
        let command = build_command(
            &app(&["wezterm", "start"]),
            Path::new("/usr/bin/wezterm"),
            None,
        );
        assert_eq!(argv(&command), vec!["/usr/bin/wezterm", "start"]);
    }

    #[test]
    fn the_cli_parses_launcher_arguments_and_any_app_arguments() {
        use crate::cli::{Cli, Command};
        use clap::Parser;
        use std::ffi::OsString;

        let unit = "app-plasma_drop-my_term-1a.scope";
        let argv = std::iter::once("plasma-drop".to_owned())
            .chain(launcher_arguments("my term", unit))
            .chain(["wezterm", "--config", "-x", "--"].map(String::from));
        let Some(Command::LaunchScoped {
            unit: parsed_unit,
            description,
            command,
        }) = Cli::try_parse_from(argv).unwrap().command
        else {
            panic!("not parsed as launch-scoped");
        };
        assert_eq!(parsed_unit, unit);
        assert_eq!(description, "PlasmaDrop app my term");
        assert_eq!(
            command,
            ["wezterm", "--config", "-x", "--"].map(OsString::from)
        );
    }

    #[test]
    fn launcher_arguments_end_before_the_command() {
        assert_eq!(
            launcher_arguments("wez term", "app-plasma_drop-wez_term-1a.scope"),
            vec![
                "launch-scoped",
                "--unit=app-plasma_drop-wez_term-1a.scope",
                "--description=PlasmaDrop app wez term",
                "--",
            ]
        );
    }

    #[test]
    fn programs_resolve_on_path_or_as_paths() {
        let dir = TempDir::new("resolve");
        let tool = dir.file("bin/tool", 0o755);
        dir.file("bin/data", 0o644);
        let path = format!("/nonexistent:{}", dir.path().join("bin").display());
        let path = Some(OsStr::new(&path));

        assert_eq!(resolve_program("tool", None, path).unwrap(), tool);
        assert_eq!(
            resolve_program(tool.to_str().unwrap(), None, None).unwrap(),
            tool
        );
        assert_eq!(
            resolve_program("bin/tool", Some(dir.path()), None).unwrap(),
            tool
        );
        assert!(
            resolve_program("missing", None, path)
                .unwrap_err()
                .to_string()
                .contains("'missing' was not found on PATH")
        );
        assert!(
            resolve_program("data", None, path).is_err(),
            "a file without execute permission is not a program"
        );
        assert!(resolve_program("/nonexistent/tool", None, path).is_err());
    }
}
