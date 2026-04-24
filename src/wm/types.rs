use crate::config::AppConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrameGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedWindow {
    #[serde(rename = "internalId")]
    pub internal_id: String,
    #[serde(rename = "desktopFileName")]
    pub desktop_file_name: String,
    #[serde(rename = "resourceClass")]
    pub resource_class: String,
    #[serde(rename = "resourceName")]
    pub resource_name: String,
    pub caption: String,
    #[serde(rename = "frameGeometry")]
    pub frame_geometry: FrameGeometry,
}

pub fn find_best_match<'a>(
    windows: &'a [ManagedWindow],
    app: &AppConfig,
) -> (Option<&'a ManagedWindow>, usize) {
    let matches: Vec<_> = windows
        .iter()
        .filter(|window| matches_window(window, app))
        .collect();
    (matches.first().copied(), matches.len())
}

pub fn matches_window(window: &ManagedWindow, app: &AppConfig) -> bool {
    let process_ok = app.process_name.as_ref().map_or_else(
        || {
            if app.window_title.is_none() {
                app.filename.as_deref().is_some_and(|filename| {
                    let filename = normalize_filename_matcher(filename);
                    [
                        window.desktop_file_name.to_ascii_lowercase(),
                        window.resource_class.to_ascii_lowercase(),
                        window.resource_name.to_ascii_lowercase(),
                    ]
                    .iter()
                    .any(|value| value == &filename)
                })
            } else {
                true
            }
        },
        |pattern| {
            pattern.is_match(&window.desktop_file_name)
                || pattern.is_match(&window.resource_class)
                || pattern.is_match(&window.resource_name)
        },
    );

    let title_ok = app
        .window_title
        .as_ref()
        .is_none_or(|pattern| pattern.is_match(&window.caption));

    process_ok && title_ok
}

fn normalize_filename_matcher(filename: &str) -> String {
    Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(filename)
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{FrameGeometry, ManagedWindow, find_best_match, matches_window};
    use crate::config::{AnimationConfig, AppConfig, AttachMode, PlacementConfig};
    use crate::hotkey::Hotkey;
    use regex::Regex;

    fn app() -> AppConfig {
        AppConfig {
            name: "terminal".into(),
            hotkey: Hotkey::parse("ctrl+grave").unwrap(),
            filename: Some("kitty".into()),
            command: vec!["kitty".into()],
            process_name: None,
            window_title: None,
            attach_mode: AttachMode::FindOrStart,
            working_directory: None,
            placement: PlacementConfig::default(),
            animation: AnimationConfig::default(),
        }
    }

    fn window(name: &str, caption: &str) -> ManagedWindow {
        ManagedWindow {
            internal_id: "{123}".into(),
            desktop_file_name: name.into(),
            resource_class: name.into(),
            resource_name: name.into(),
            caption: caption.into(),
            frame_geometry: FrameGeometry {
                x: 0,
                y: 0,
                width: 100,
                height: 50,
            },
        }
    }

    #[test]
    fn exact_filename_match_is_case_insensitive() {
        let mut app = app();
        app.filename = Some("KiTtY".into());
        assert!(matches_window(&window("kitty", "shell"), &app));
    }

    #[test]
    fn absolute_filename_matches_by_basename() {
        let mut app = app();
        app.filename = Some("/usr/bin/dolphin".into());
        assert!(matches_window(&window("dolphin", "files"), &app));
    }

    #[test]
    fn process_name_regex_matches_identity_fields() {
        let mut app = app();
        app.process_name = Some(Regex::new("(?i)kit.*").unwrap());
        assert!(matches_window(&window("kitty", "shell"), &app));
    }

    #[test]
    fn title_regex_must_match_when_present() {
        let mut app = app();
        app.window_title = Some(Regex::new("(?i)project").unwrap());
        assert!(matches_window(&window("kitty", "Project"), &app));
        assert!(!matches_window(&window("kitty", "shell"), &app));
    }

    #[test]
    fn first_match_is_selected_and_ambiguity_is_reported() {
        let app = app();
        let windows = vec![window("kitty", "one"), window("kitty", "two")];
        let (selected, count) = find_best_match(&windows, &app);
        assert_eq!(count, 2);
        assert_eq!(selected.unwrap().caption, "one");
    }

    #[test]
    fn process_name_and_window_title_must_both_match() {
        let mut app = app();
        app.process_name = Some(Regex::new("(?i)kitty").unwrap());
        app.window_title = Some(Regex::new("(?i)project").unwrap());

        assert!(matches_window(&window("kitty", "Project"), &app));
        assert!(!matches_window(&window("kitty", "Shell"), &app));
        assert!(!matches_window(&window("dolphin", "Project"), &app));
    }

    #[test]
    fn title_only_matching_works_without_filename_or_process_name() {
        let mut app = app();
        app.filename = None;
        app.process_name = None;
        app.window_title = Some(Regex::new("(?i)dolphin").unwrap());

        assert!(matches_window(&window("something-else", "Dolphin"), &app));
        assert!(!matches_window(&window("something-else", "Terminal"), &app));
    }
}
