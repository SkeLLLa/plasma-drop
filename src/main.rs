#![doc = include_str!("../docs/index.md")]

#[doc = include_str!("../docs/getting-started.md")]
pub mod getting_started {}

#[doc = include_str!("../docs/configuration.md")]
pub mod configuration {}

#[doc = include_str!("../docs/development.md")]
pub mod development {}

#[allow(clippy::doc_markdown)]
mod animation;
mod app_registry;
mod cli;
mod config;
mod embedded_assets;
mod hotkey;
mod screen;
mod toggle;
mod wm;

use anyhow::{Context, Result};
use app_registry::{AppRegistry, ManagedApp};
use clap::Parser;
use cli::{Cli, Command};
use config::{Config, default_config_path};
use screen::parse_support_information;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use toggle::ToggleService;
use tokio::sync::Mutex;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use wm::HOTKEY_PREFIX;
use wm::kwin::client::KWinClient;
use wm::kwin::script::ensure_script_file;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(command) = cli.command {
        match command {
            Command::Init { systemd, force } => {
                init_user_files(systemd, force)?;
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
        }
    }

    let config_path = cli.config.clone().unwrap_or(default_config_path()?);
    let config = Config::load(&config_path)?;
    init_tracing(&cli, &config);
    info!("loaded config from {}", config_path.display());

    let (kwin, mut hotkey_rx) = KWinClient::connect().await?;
    kwin.ensure_compatibility().await?;

    let script_path = ensure_script_file().await?;
    let script_plugin = kwin.load_script(&script_path).await?;
    info!("loaded KWin script '{}'", script_plugin);

    kwin.cleanup_shortcuts().await?;

    let screens = {
        let text = kwin.support_information_text().await?;
        parse_support_information(&text)?
    };
    for screen in &screens {
        info!(
            "detected screen {} '{}' at {},{} {}x{}",
            screen.index, screen.name, screen.x, screen.y, screen.width, screen.height
        );
    }

    let managed_apps = build_registry_entries(&config);
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
    }
    let registry = Arc::new(Mutex::new(AppRegistry::new(managed_apps)));

    let kwin = Arc::new(kwin);
    let toggle_service = ToggleService::new(registry.clone(), kwin.clone(), screens);

    let result = loop {
        tokio::select! {
            maybe_hotkey = hotkey_rx.recv() => {
                match maybe_hotkey {
                    Some(shortcut_id) => {
                        if let Err(error) = toggle_service.handle_shortcut(&shortcut_id).await {
                            warn!("hotkey '{}' failed: {error:#}", shortcut_id);
                        }
                    }
                    None => break Ok(()),
                }
            }
            signal = shutdown_signal() => {
                match signal {
                    Ok(()) => {
                        info!("shutdown signal received");
                        break Ok(());
                    }
                    Err(error) => break Err(error),
                }
            }
        }
    };

    if let Err(error) = toggle_service.restore_tracked_windows_on_shutdown().await {
        warn!("failed to restore tracked windows on shutdown: {error:#}");
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
            restore_geometry: None,
            restore_no_border: None,
            visible: false,
            shortcut_id: format!("{HOTKEY_PREFIX}{}_{}", app.sanitized_name(), idx + 1),
        })
        .collect()
}

fn init_user_files(with_systemd: bool, force: bool) -> Result<()> {
    let config_path = default_config_path()?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory '{}'", parent.display()))?;
    }
    write_file(&config_path, embedded_assets::EXAMPLE_CONFIG, force)?;
    println!("wrote {}", config_path.display());

    if with_systemd {
        let service_path = default_systemd_service_path()?;
        if let Some(parent) = service_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create systemd directory '{}'", parent.display())
            })?;
        }
        write_file(&service_path, embedded_assets::USER_SYSTEMD_SERVICE, force)?;
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

fn write_file(path: &Path, contents: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        anyhow::bail!(
            "'{}' already exists; rerun with --force to overwrite",
            path.display()
        );
    }
    fs::write(path, contents).with_context(|| format!("failed to write '{}'", path.display()))
}

async fn shutdown_signal() -> Result<()> {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let terminate = async {
        let mut signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .context("failed to install SIGTERM handler")?;
        signal.recv().await;
        Result::<()>::Ok(())
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<Result<()>>();

    tokio::select! {
        result = ctrl_c => {
            result.context("failed while waiting for Ctrl+C")?;
            Ok(())
        }
        result = terminate => result,
    }
}
