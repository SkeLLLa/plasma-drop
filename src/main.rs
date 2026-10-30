#![doc = include_str!("../docs/index.md")]

#[doc = include_str!("../docs/getting-started.md")]
pub mod getting_started {}

#[doc = include_str!("../docs/configuration.md")]
pub mod configuration {}

#[doc = include_str!("../docs/development.md")]
pub mod development {}

mod animation;
mod app_registry;
mod cli;
mod config;
mod embedded_assets;
mod hotkey;
mod notify;
mod scope;
mod screen;
#[cfg(test)]
mod test_support;
mod toggle;
mod wm;

use anyhow::{Context, Result, bail};
use app_registry::{AppRegistry, ManagedApp, RestoreState};
use clap::Parser;
use cli::{Cli, Command};
use config::{Config, default_config_path};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use toggle::ToggleService;
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use wm::kwin::client::{HotkeyStatus, KWinClient};
use wm::kwin::script::ensure_script_file;
use wm::{HOTKEY_PREFIX, WindowManager};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();
    if let Some(command) = cli.command.take() {
        match command {
            Command::Init { systemd, force } => {
                init_user_files(
                    if systemd {
                        SystemdUnit::Install
                    } else {
                        SystemdUnit::Skip
                    },
                    if force {
                        Overwrite::Replace
                    } else {
                        Overwrite::Refuse
                    },
                )?;
                return Ok(());
            }
            Command::PrintExampleConfig => {
                print!("{}", embedded_assets::EXAMPLE_CONFIG);
                return Ok(());
            }
            Command::PrintSystemdService => {
                print!("{}", embedded_assets::USER_SYSTEMD_SERVICE);
                return Ok(());
            }
            Command::LaunchScoped {
                unit,
                description,
                command,
            } => match scope::launch_scoped(&unit, &description, &command).await? {},
        }
    }

    let config_path = match cli.config.take() {
        Some(path) => path,
        None => default_config_path()?,
    };
    let config = Config::load(&config_path)?;
    init_tracing(&cli, &config);
    info!("loaded config from {}", config_path.display());

    let (kwin, mut hotkey_rx, mut focus_rx) = KWinClient::connect().await?;

    let script_path = ensure_script_file().await?;
    let script_plugin = kwin.load_script(&script_path).await?;
    info!("loaded KWin script '{}'", script_plugin);

    kwin.cleanup_shortcuts().await?;

    let screens = kwin.screens().await?;
    if screens.is_empty() {
        bail!("KWin reported no screens");
    }
    for screen in &screens {
        info!(
            "detected screen {} '{}' at {},{} {}x{}",
            screen.index, screen.name, screen.x, screen.y, screen.width, screen.height
        );
    }

    let managed_apps = build_registry_entries(&config);
    let mut hotkey_problems = Vec::new();
    for app in &managed_apps {
        kwin.register_hotkey(
            &app.shortcut_id,
            &format!("PlasmaDrop hotkey - {}", app.config.name),
            app.config.hotkey.sequence(),
        )
        .await
        .with_context(|| format!("failed to register hotkey for '{}'", app.config.name))?;
        info!(
            hotkey = %app.config.hotkey.sequence(),
            app = %app.config.name,
            "registered hotkey"
        );
        match kwin.hotkey_status(&app.shortcut_id).await {
            Ok(status) => {
                if let Some(problem) =
                    hotkey_problem(&app.config.name, app.config.hotkey.raw(), &status)
                {
                    error!("{problem}");
                    hotkey_problems.push(problem);
                }
            }
            Err(error) => warn!(
                "failed to check hotkey for '{}': {error:#}",
                app.config.name
            ),
        }
    }
    if !hotkey_problems.is_empty() {
        notify::spawn(
            kwin.connection().clone(),
            "Some plasma-drop hotkeys don't work".to_string(),
            hotkey_problems.join("\n\n"),
        );
    }
    let registry = Arc::new(Mutex::new(AppRegistry::new(managed_apps)));

    let kwin = Arc::new(kwin);
    let toggle_service = ToggleService::new(
        Arc::clone(&registry),
        Arc::<KWinClient>::clone(&kwin),
        screens,
    );
    // Both futures live across loop iterations, so a SIGTERM or bridge loss that fires while
    // a hotkey is being handled stays recorded.
    let shutdown = shutdown_signal()?;
    tokio::pin!(shutdown);
    let bridge_loss = kwin.wait_for_bridge_loss();
    tokio::pin!(bridge_loss);
    // Waits for spawned apps' windows, so a slow app start does not stall the loop.
    let mut launches = JoinSet::new();

    // The second value says whether the script bridge was lost.
    let (result, bridge_lost) = loop {
        tokio::select! {
            maybe_hotkey = hotkey_rx.recv() => {
                match maybe_hotkey {
                    Some((shortcut_id, pressed_at)) => {
                        match toggle_service.handle_shortcut(&shortcut_id, pressed_at).await {
                            Ok(Some(launch)) => {
                                let service = toggle_service.clone();
                                launches.spawn(async move { service.wait_for_launch(launch).await });
                            }
                            Ok(None) => {}
                            Err(error) => warn!("hotkey '{}' failed: {error:#}", shortcut_id),
                        }
                    }
                    None => break (Ok(()), false),
                }
            }
            maybe_focus = focus_rx.recv() => {
                match maybe_focus {
                    Some(mut latest) => {
                        // Only the newest activation of a burst matters; `handle_focus_change`
                        // confirms anything but our own window with KWin.
                        while let Ok(next) = focus_rx.try_recv() {
                            latest = next;
                        }
                        if let Err(error) = toggle_service.handle_focus_change(latest).await {
                            warn!("failed to hide app after focus change: {error:#}");
                        }
                    }
                    None => break (Ok(()), false),
                }
            }
            Some(joined) = launches.join_next(), if !launches.is_empty() => {
                match joined {
                    Ok(launch) => {
                        if let Err(error) = toggle_service.finish_launch(launch).await {
                            warn!("failed to show started app: {error:#}");
                        }
                    }
                    Err(error) => warn!("waiting for a started app's window failed: {error}"),
                }
            }
            signal = &mut shutdown => {
                match signal {
                    Ok(()) => {
                        info!("shutdown signal received");
                        break (Ok(()), false);
                    }
                    Err(error) => break (Err(error), false),
                }
            }
            error = &mut bridge_loss => {
                break (
                    Err(error.context("lost the KWin script bridge; exiting so the service restarts")),
                    true,
                );
            }
        }
    };

    // Restoring goes through the script bridge; with the bridge gone each command would
    // only wait out its timeout.
    if !bridge_lost {
        toggle_service.restore_tracked_windows_on_shutdown().await;
    }
    if let Err(error) = kwin.cleanup_shortcuts().await {
        warn!("failed to cleanup shortcuts: {error:#}");
    }
    if let Err(error) = kwin.unload_script(&script_plugin).await {
        warn!("failed to unload KWin script: {error:#}");
    }

    result
}

fn init_tracing(cli: &Cli, config: &Config) {
    let level = cli.log_level.as_deref().map_or_else(
        || match cli.verbose {
            0 => config.log_level.clone(),
            1 => "debug".to_string(),
            _ => "trace".to_string(),
        },
        ToString::to_string,
    );
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn build_registry_entries(config: &Config) -> Vec<ManagedApp> {
    config
        .apps
        .iter()
        .enumerate()
        .map(|(idx, app)| ManagedApp {
            config: app.clone(),
            tracked_window_id: None,
            restore: RestoreState::default(),
            hidden_full_screen: false,
            shortcut_id: format!("{HOTKEY_PREFIX}{}_{}", app.sanitized_name(), idx + 1),
        })
        .collect()
}

/// Why an app's hotkey does not fire, for the log and a desktop notification.
fn hotkey_problem(app: &str, hotkey: &str, status: &HotkeyStatus) -> Option<String> {
    match status {
        HotkeyStatus::Active => None,
        HotkeyStatus::NotAccepted => Some(format!(
            "Hotkey '{hotkey}' for '{app}' doesn't work: KWin didn't accept it."
        )),
        HotkeyStatus::Shadowed(rivals) => {
            let rivals = rivals
                .iter()
                .map(|rival| format!("'{}' ({})", rival.action, rival.component))
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!(
                "Hotkey '{hotkey}' for '{app}' doesn't work while {rivals} uses the same key. \
                 Remove it there in System Settings > Keyboard > Shortcuts; plasma-drop picks \
                 up the change without a restart."
            ))
        }
    }
}

/// Whether `init` also writes the user systemd unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystemdUnit {
    Skip,
    Install,
}

/// What `init` does with a file that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Overwrite {
    /// Fail, pointing at `--force`.
    Refuse,
    /// Overwrite it (`--force`).
    Replace,
}

fn init_user_files(systemd: SystemdUnit, overwrite: Overwrite) -> Result<()> {
    let config_path = default_config_path()?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory '{}'", parent.display()))?;
    }
    write_file(&config_path, embedded_assets::EXAMPLE_CONFIG, overwrite)?;
    println!("wrote {}", config_path.display());

    if systemd == SystemdUnit::Install {
        let service_path = default_systemd_service_path()?;
        if let Some(parent) = service_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create systemd directory '{}'", parent.display())
            })?;
        }
        write_file(
            &service_path,
            embedded_assets::USER_SYSTEMD_SERVICE,
            overwrite,
        )?;
        println!("wrote {}", service_path.display());
    }

    Ok(())
}

fn default_systemd_service_path() -> Result<std::path::PathBuf> {
    let home = std::env::var_os("HOME").context("HOME environment variable is not set")?;
    Ok(Path::new(&home)
        .join(".config")
        .join("systemd")
        .join("user")
        .join("plasma-drop.service"))
}

fn write_file(path: &Path, contents: &str, overwrite: Overwrite) -> Result<()> {
    if overwrite == Overwrite::Refuse && path.exists() {
        bail!(
            "'{}' already exists; rerun with --force to overwrite",
            path.display()
        );
    }
    fs::write(path, contents).with_context(|| format!("failed to write '{}'", path.display()))
}

/// Installs the shutdown signal handlers now and returns a future that resolves on
/// Ctrl+C or SIGTERM. Installing eagerly means a signal that arrives while the returned
/// future is not being polled is still observed on the next poll.
fn shutdown_signal() -> Result<impl Future<Output = Result<()>>> {
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("failed to install SIGTERM handler")?;

    Ok(async move {
        #[cfg(unix)]
        let terminate = sigterm.recv();
        #[cfg(not(unix))]
        let terminate = std::future::pending::<Option<()>>();

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.context("failed while waiting for Ctrl+C")
            }
            _ = terminate => Ok(()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::hotkey_problem;
    use crate::wm::kwin::client::{HotkeyStatus, RivalShortcut};

    #[test]
    fn active_hotkey_has_no_problem() {
        assert_eq!(hotkey_problem("term", "super", &HotkeyStatus::Active), None);
    }

    #[test]
    fn shadowed_hotkey_names_the_rival() {
        let status = HotkeyStatus::Shadowed(vec![RivalShortcut {
            action: "Activate Application Launcher".into(),
            component: "plasmashell".into(),
        }]);
        let problem = hotkey_problem("term", "super", &status).unwrap();
        assert!(problem.starts_with("Hotkey 'super' for 'term' doesn't work while"));
        assert!(problem.contains("'Activate Application Launcher' (plasmashell)"));
        assert!(problem.contains("System Settings > Keyboard > Shortcuts"));
    }

    #[test]
    fn rejected_hotkey_says_so() {
        let problem = hotkey_problem("term", "super", &HotkeyStatus::NotAccepted).unwrap();
        assert!(problem.contains("KWin didn't accept it"));
    }
}
