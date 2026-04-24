use crate::hotkey::Hotkey;
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachMode {
    Find,
    FindOrStart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementMetric {
    Percent(i16),
    Pixels(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementPosition {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationStyle {
    None,
    Slide,
    Fade,
    SlideFade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationEasing {
    Linear,
    EaseOut,
    EaseInOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementConfig {
    pub width: PlacementMetric,
    pub height: PlacementMetric,
    pub position: PlacementPosition,
    pub offset_x: PlacementMetric,
    pub offset_y: PlacementMetric,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationConfig {
    pub style: AnimationStyle,
    pub easing: AnimationEasing,
    pub duration_ms: u16,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub apps: Vec<AppConfig>,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub name: String,
    pub hotkey: Hotkey,
    pub filename: Option<String>,
    pub command: Vec<String>,
    pub process_name: Option<Regex>,
    pub window_title: Option<Regex>,
    pub attach_mode: AttachMode,
    pub working_directory: Option<PathBuf>,
    pub placement: PlacementConfig,
    pub animation: AnimationConfig,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(rename = "app", default)]
    apps: Vec<RawAppConfig>,
}

#[derive(Debug, Deserialize)]
struct RawAppConfig {
    name: String,
    hotkey: String,
    filename: Option<String>,
    command: Option<Vec<String>>,
    process_name: Option<String>,
    window_title: Option<String>,
    attach_mode: Option<String>,
    arguments: Option<Vec<String>>,
    working_directory: Option<PathBuf>,
    placement: Option<RawPlacementConfig>,
    animation: Option<RawAnimationConfig>,
}

#[derive(Debug, Deserialize)]
struct RawPlacementConfig {
    width: Option<String>,
    height: Option<String>,
    position: Option<String>,
    offset_x: Option<String>,
    offset_y: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAnimationConfig {
    style: Option<String>,
    easing: Option<String>,
    duration_ms: Option<u16>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read config '{}'", path.display()))?;
        let raw: RawConfig = toml::from_str(&text)
            .with_context(|| format!("failed to parse config '{}'", path.display()))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self> {
        if raw.apps.is_empty() {
            bail!("config must contain at least one [[app]] entry");
        }

        let mut names = HashSet::new();
        let mut hotkeys = HashSet::new();
        let mut apps = Vec::with_capacity(raw.apps.len());

        for app in raw.apps {
            let name = app.name.trim().to_string();
            if name.is_empty() {
                bail!("app name must not be empty");
            }
            if name.len() > 64 {
                bail!("app name '{name}' exceeds 64 characters");
            }
            if !names.insert(name.clone()) {
                bail!("duplicate app name '{name}'");
            }

            let hotkey = Hotkey::parse(&app.hotkey)
                .with_context(|| format!("invalid hotkey for app '{name}'"))?;
            if !hotkeys.insert(hotkey.sequence().to_string()) {
                bail!("duplicate hotkey '{}'", hotkey.raw());
            }

            if app.filename.is_none() && app.process_name.is_none() && app.window_title.is_none() {
                bail!(
                    "app '{name}' must set at least one of filename, process_name, or window_title"
                );
            }

            let attach_mode = match app.attach_mode.as_deref().unwrap_or("find-or-start") {
                "find" => AttachMode::Find,
                "find-or-start" => AttachMode::FindOrStart,
                other => bail!("app '{name}' has invalid attach_mode '{other}'"),
            };

            let arguments = app.arguments.unwrap_or_default();
            let command = build_spawn_command(
                &name,
                app.command.as_deref(),
                &arguments,
                app.filename.as_deref(),
            )?;
            let process_name =
                compile_optional_regex(&name, "process_name", app.process_name.as_deref())?;
            let window_title =
                compile_optional_regex(&name, "window_title", app.window_title.as_deref())?;
            let placement = PlacementConfig::from_raw(app.placement.as_ref(), &name)?;
            let animation = AnimationConfig::from_raw(app.animation.as_ref(), &name)?;

            if let Some(dir) = app.working_directory.as_ref() {
                if !dir.is_absolute() {
                    bail!(
                        "app '{name}' has non-absolute working_directory '{}'",
                        dir.display()
                    );
                }
                if !dir.exists() {
                    bail!(
                        "app '{name}' working_directory does not exist: '{}'",
                        dir.display()
                    );
                }
            }

            apps.push(AppConfig {
                name,
                hotkey,
                filename: app.filename,
                command,
                process_name,
                window_title,
                attach_mode,
                working_directory: app.working_directory,
                placement,
                animation,
            });
        }

        Ok(Self { apps })
    }
}

pub fn default_config_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME environment variable is not set")?;
    Ok(PathBuf::from(home).join(".config/plasma-drop/config.toml"))
}

fn compile_optional_regex(
    app_name: &str,
    field: &str,
    pattern: Option<&str>,
) -> Result<Option<Regex>> {
    pattern
        .map(|raw| {
            Regex::new(&format!("(?i){raw}"))
                .with_context(|| format!("app '{app_name}' has invalid {field} regex"))
        })
        .transpose()
}

fn build_spawn_command(
    app_name: &str,
    explicit_command: Option<&[String]>,
    arguments: &[String],
    filename: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(command) = explicit_command {
        if command.is_empty() {
            bail!("app '{app_name}' has empty command");
        }
        if command[0].trim().is_empty() {
            bail!("app '{app_name}' has invalid command program");
        }
        if !arguments.is_empty() {
            bail!("app '{app_name}' cannot set both command and arguments");
        }
        return Ok(command.to_vec());
    }

    let mut command = Vec::with_capacity(usize::from(filename.is_some()) + arguments.len());
    if let Some(filename) = filename {
        command.push(filename.to_string());
    }
    command.extend(arguments.iter().cloned());
    Ok(command)
}

impl AppConfig {
    pub fn sanitized_name(&self) -> String {
        self.name
            .chars()
            .map(|ch| {
                let lower = ch.to_ascii_lowercase();
                if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
                    lower
                } else {
                    '_'
                }
            })
            .collect()
    }
}

impl PlacementConfig {
    fn from_raw(raw: Option<&RawPlacementConfig>, app_name: &str) -> Result<Self> {
        let Some(raw) = raw else {
            return Ok(Self::default());
        };

        Ok(Self {
            width: parse_metric(
                app_name,
                "width",
                raw.width.as_deref().unwrap_or("100%"),
                MetricKind::Size,
            )?,
            height: parse_metric(
                app_name,
                "height",
                raw.height.as_deref().unwrap_or("100%"),
                MetricKind::Size,
            )?,
            position: PlacementPosition::parse(
                app_name,
                raw.position.as_deref().unwrap_or("top-left"),
            )?,
            offset_x: parse_metric(
                app_name,
                "offset_x",
                raw.offset_x.as_deref().unwrap_or("0px"),
                MetricKind::Offset,
            )?,
            offset_y: parse_metric(
                app_name,
                "offset_y",
                raw.offset_y.as_deref().unwrap_or("0px"),
                MetricKind::Offset,
            )?,
        })
    }
}

impl AnimationConfig {
    fn from_raw(raw: Option<&RawAnimationConfig>, app_name: &str) -> Result<Self> {
        let Some(raw) = raw else {
            return Ok(Self::default());
        };

        let style = AnimationStyle::parse(app_name, raw.style.as_deref().unwrap_or("none"))?;
        let easing = AnimationEasing::parse(app_name, raw.easing.as_deref().unwrap_or("ease-out"))?;
        let duration_ms = raw.duration_ms.unwrap_or(150);

        if duration_ms > 2_000 {
            bail!("app '{app_name}' has animation.duration_ms larger than 2000: '{duration_ms}'");
        }

        Ok(Self {
            style,
            easing,
            duration_ms,
        })
    }
}

impl Default for PlacementConfig {
    fn default() -> Self {
        Self {
            width: PlacementMetric::Percent(100),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::TopLeft,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
        }
    }
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            style: AnimationStyle::None,
            easing: AnimationEasing::EaseOut,
            duration_ms: 150,
        }
    }
}

impl PlacementPosition {
    fn parse(app_name: &str, raw: &str) -> Result<Self> {
        match raw {
            "top-left" => Ok(Self::TopLeft),
            "top" => Ok(Self::Top),
            "top-right" => Ok(Self::TopRight),
            "left" => Ok(Self::Left),
            "center" => Ok(Self::Center),
            "right" => Ok(Self::Right),
            "bottom-left" => Ok(Self::BottomLeft),
            "bottom" => Ok(Self::Bottom),
            "bottom-right" => Ok(Self::BottomRight),
            other => bail!("app '{app_name}' has invalid position '{other}'"),
        }
    }
}

impl AnimationStyle {
    fn parse(app_name: &str, raw: &str) -> Result<Self> {
        match raw {
            "none" => Ok(Self::None),
            "slide" => Ok(Self::Slide),
            "fade" => Ok(Self::Fade),
            "slide-fade" => Ok(Self::SlideFade),
            other => bail!("app '{app_name}' has invalid animation.style '{other}'"),
        }
    }
}

impl AnimationEasing {
    fn parse(app_name: &str, raw: &str) -> Result<Self> {
        match raw {
            "linear" => Ok(Self::Linear),
            "ease-out" => Ok(Self::EaseOut),
            "ease-in-out" => Ok(Self::EaseInOut),
            other => bail!("app '{app_name}' has invalid animation.easing '{other}'"),
        }
    }
}

#[derive(Clone, Copy)]
enum MetricKind {
    Size,
    Offset,
}

fn parse_metric(
    app_name: &str,
    field: &str,
    raw: &str,
    kind: MetricKind,
) -> Result<PlacementMetric> {
    if raw.is_empty() || raw.trim() != raw {
        bail!("app '{app_name}' has invalid {field} metric '{raw}'");
    }

    let (value, unit) = if let Some(value) = raw.strip_suffix('%') {
        (value, "%")
    } else if let Some(value) = raw.strip_suffix("px") {
        (value, "px")
    } else {
        bail!("app '{app_name}' has invalid {field} metric '{raw}'");
    };

    if value.is_empty() || value.contains(char::is_whitespace) {
        bail!("app '{app_name}' has invalid {field} metric '{raw}'");
    }

    let number = value
        .parse::<i32>()
        .with_context(|| format!("app '{app_name}' has invalid {field} metric '{raw}'"))?;

    if matches!(kind, MetricKind::Size) && number <= 0 {
        bail!("app '{app_name}' has non-positive {field} metric '{raw}'");
    }

    match unit {
        "%" => {
            if matches!(kind, MetricKind::Size) && number > 100 {
                bail!("app '{app_name}' has {field} percentage larger than 100: '{raw}'");
            }
            if number < i32::from(i16::MIN) || number > i32::from(i16::MAX) {
                bail!("app '{app_name}' has out-of-range {field} metric '{raw}'");
            }
            Ok(PlacementMetric::Percent(
                i16::try_from(number).expect("validated metric should fit within i16"),
            ))
        }
        "px" => Ok(PlacementMetric::Pixels(number)),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnimationConfig, AnimationEasing, AnimationStyle, Config, PlacementConfig, PlacementMetric,
        PlacementPosition, RawAppConfig, RawConfig,
    };
    use std::path::PathBuf;

    #[allow(clippy::missing_const_for_fn)]
    fn make_raw(apps: Vec<RawAppConfig>) -> RawConfig {
        RawConfig { apps }
    }

    fn app(name: &str, hotkey: &str) -> RawAppConfig {
        RawAppConfig {
            name: name.to_string(),
            hotkey: hotkey.to_string(),
            filename: Some("kitty".into()),
            command: None,
            process_name: None,
            window_title: None,
            attach_mode: None,
            arguments: None,
            working_directory: None,
            placement: None,
            animation: None,
        }
    }

    #[test]
    fn rejects_duplicate_name() {
        let err = Config::from_raw(make_raw(vec![
            app("terminal", "ctrl+grave"),
            app("terminal", "ctrl+e"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("duplicate app name"));
    }

    #[test]
    fn rejects_duplicate_hotkey() {
        let err = Config::from_raw(make_raw(vec![
            app("terminal", "ctrl+grave"),
            app("files", "ctrl+grave"),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("duplicate hotkey"));
    }

    #[test]
    fn rejects_missing_matcher() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.filename = None;
        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("at least one of"));
    }

    #[test]
    fn rejects_bad_regex() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.process_name = Some("[".into());
        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("invalid process_name regex"));
    }

    #[test]
    fn rejects_relative_working_directory() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.working_directory = Some(PathBuf::from("relative"));
        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("non-absolute working_directory"));
    }

    #[test]
    fn rejects_name_longer_than_sixty_four_characters() {
        let long_name = "a".repeat(65);
        let err = Config::from_raw(make_raw(vec![app(&long_name, "ctrl+grave")])).unwrap_err();
        assert!(err.to_string().contains("exceeds 64 characters"));
    }

    #[test]
    fn omitted_placement_uses_defaults() {
        let config = Config::from_raw(make_raw(vec![app("terminal", "ctrl+grave")])).unwrap();
        assert_eq!(config.apps[0].placement, PlacementConfig::default());
        assert_eq!(config.apps[0].animation, AnimationConfig::default());
    }

    #[test]
    fn filename_and_arguments_build_spawn_command() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.arguments = Some(vec!["--single-instance".into()]);

        let config = Config::from_raw(make_raw(vec![raw])).unwrap();
        assert_eq!(
            config.apps[0].command,
            vec!["kitty".to_string(), "--single-instance".to_string()]
        );
    }

    #[test]
    fn explicit_command_overrides_spawn_target() {
        let mut raw = app("browser", "ctrl+b");
        raw.filename = Some("chromium".into());
        raw.command = Some(vec![
            "/usr/bin/flatpak".into(),
            "run".into(),
            "--branch=stable".into(),
            "io.github.ungoogled_software.ungoogled_chromium".into(),
        ]);

        let config = Config::from_raw(make_raw(vec![raw])).unwrap();
        assert_eq!(
            config.apps[0].command,
            vec![
                "/usr/bin/flatpak".to_string(),
                "run".to_string(),
                "--branch=stable".to_string(),
                "io.github.ungoogled_software.ungoogled_chromium".to_string(),
            ]
        );
        assert_eq!(config.apps[0].filename.as_deref(), Some("chromium"));
    }

    #[test]
    fn rejects_empty_command() {
        let mut raw = app("browser", "ctrl+b");
        raw.command = Some(Vec::new());

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("empty command"));
    }

    #[test]
    fn rejects_command_and_arguments_together() {
        let mut raw = app("browser", "ctrl+b");
        raw.command = Some(vec!["/usr/bin/flatpak".into(), "run".into()]);
        raw.arguments = Some(vec!["--verbose".into()]);

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("both command and arguments"));
    }

    #[test]
    fn accepts_percent_and_pixel_metrics() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.placement = Some(super::RawPlacementConfig {
            width: Some("50%".into()),
            height: Some("640px".into()),
            position: Some("right".into()),
            offset_x: Some("-2%".into()),
            offset_y: Some("16px".into()),
        });

        let config = Config::from_raw(make_raw(vec![raw])).unwrap();
        assert_eq!(config.apps[0].placement.width, PlacementMetric::Percent(50));
        assert_eq!(
            config.apps[0].placement.height,
            PlacementMetric::Pixels(640)
        );
        assert_eq!(config.apps[0].placement.position, PlacementPosition::Right);
        assert_eq!(
            config.apps[0].placement.offset_x,
            PlacementMetric::Percent(-2)
        );
        assert_eq!(
            config.apps[0].placement.offset_y,
            PlacementMetric::Pixels(16)
        );
    }

    #[test]
    fn rejects_malformed_metric() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.placement = Some(super::RawPlacementConfig {
            width: Some("50".into()),
            height: None,
            position: None,
            offset_x: None,
            offset_y: None,
        });

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("invalid width metric"));
    }

    #[test]
    fn rejects_zero_sized_percent_metric() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.placement = Some(super::RawPlacementConfig {
            width: Some("0%".into()),
            height: None,
            position: None,
            offset_x: None,
            offset_y: None,
        });

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("non-positive width metric"));
    }

    #[test]
    fn rejects_zero_sized_pixel_metric() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.placement = Some(super::RawPlacementConfig {
            width: None,
            height: Some("0px".into()),
            position: None,
            offset_x: None,
            offset_y: None,
        });

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("non-positive height metric"));
    }

    #[test]
    fn rejects_unknown_position() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.placement = Some(super::RawPlacementConfig {
            width: None,
            height: None,
            position: Some("right-half".into()),
            offset_x: None,
            offset_y: None,
        });

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("invalid position"));
    }

    #[test]
    fn accepts_animation_config() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.animation = Some(super::RawAnimationConfig {
            style: Some("slide-fade".into()),
            easing: Some("ease-in-out".into()),
            duration_ms: Some(220),
        });

        let config = Config::from_raw(make_raw(vec![raw])).unwrap();
        assert_eq!(config.apps[0].animation.style, AnimationStyle::SlideFade);
        assert_eq!(config.apps[0].animation.easing, AnimationEasing::EaseInOut);
        assert_eq!(config.apps[0].animation.duration_ms, 220);
    }

    #[test]
    fn rejects_invalid_animation_style() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.animation = Some(super::RawAnimationConfig {
            style: Some("bounce".into()),
            easing: None,
            duration_ms: None,
        });

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err.to_string().contains("invalid animation.style"));
    }

    #[test]
    fn rejects_animation_duration_above_limit() {
        let mut raw = app("terminal", "ctrl+grave");
        raw.animation = Some(super::RawAnimationConfig {
            style: Some("slide".into()),
            easing: None,
            duration_ms: Some(2_001),
        });

        let err = Config::from_raw(make_raw(vec![raw])).unwrap_err();
        assert!(err
            .to_string()
            .contains("animation.duration_ms larger than 2000"));
    }
}
