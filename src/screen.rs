use crate::config::{PlacementConfig, PlacementMetric, PlacementPosition};
use crate::wm::FrameGeometry;
use anyhow::{Context, Result, bail};
use regex::Regex;
use std::sync::OnceLock;

fn screen_header_re() -> &'static Regex {
    static SCREEN_HEADER_RE: OnceLock<Regex> = OnceLock::new();
    SCREEN_HEADER_RE
        .get_or_init(|| Regex::new(r"(?im)^Screen (\d+):$").expect("valid screen header regex"))
}

fn screen_name_re() -> &'static Regex {
    static SCREEN_NAME_RE: OnceLock<Regex> = OnceLock::new();
    SCREEN_NAME_RE
        .get_or_init(|| Regex::new(r"(?im)^Name: (.+)$").expect("valid screen name regex"))
}

fn screen_geometry_re() -> &'static Regex {
    static SCREEN_GEOMETRY_RE: OnceLock<Regex> = OnceLock::new();
    SCREEN_GEOMETRY_RE.get_or_init(|| {
        Regex::new(r"(?im)^Geometry: (-?\d+),(-?\d+),(\d+)x(\d+)$")
            .expect("valid screen geometry regex")
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenInfo {
    pub index: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl ScreenInfo {
    pub const fn contains_point(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    pub fn overlap_area(&self, rect: &FrameGeometry) -> i64 {
        let left = self.x.max(rect.x);
        let top = self.y.max(rect.y);
        let right = (self.x + self.width).min(rect.x + rect.width);
        let bottom = (self.y + self.height).min(rect.y + rect.height);

        if right <= left || bottom <= top {
            return 0;
        }

        i64::from(right - left) * i64::from(bottom - top)
    }

    pub fn placement_rect(&self, placement: &PlacementConfig) -> FrameGeometry {
        let width = Self::resolve_metric(&placement.width, self.width);
        let height = Self::resolve_metric(&placement.height, self.height);

        let base_x = match placement.position {
            PlacementPosition::TopLeft
            | PlacementPosition::Left
            | PlacementPosition::BottomLeft => self.x,
            PlacementPosition::Top | PlacementPosition::Center | PlacementPosition::Bottom => {
                self.x + (self.width - width) / 2
            }
            PlacementPosition::TopRight
            | PlacementPosition::Right
            | PlacementPosition::BottomRight => self.x + self.width - width,
        };

        let base_y = match placement.position {
            PlacementPosition::TopLeft | PlacementPosition::Top | PlacementPosition::TopRight => {
                self.y
            }
            PlacementPosition::Left | PlacementPosition::Center | PlacementPosition::Right => {
                self.y + (self.height - height) / 2
            }
            PlacementPosition::BottomLeft
            | PlacementPosition::Bottom
            | PlacementPosition::BottomRight => self.y + self.height - height,
        };

        let offset_x = Self::resolve_metric(&placement.offset_x, self.width);
        let offset_y = Self::resolve_metric(&placement.offset_y, self.height);

        let (x, width) = Self::place_axis(self.x, self.width, width, base_x, offset_x);
        let (y, height) = Self::place_axis(self.y, self.height, height, base_y, offset_y);

        FrameGeometry {
            x,
            y,
            width,
            height,
        }
    }

    pub fn validate_placement(&self, placement: &PlacementConfig) -> Result<()> {
        let width = Self::resolve_metric(&placement.width, self.width);
        let height = Self::resolve_metric(&placement.height, self.height);

        if width < 1 {
            bail!("placement width resolved to {width}, must be at least 1");
        }
        if height < 1 {
            bail!("placement height resolved to {height}, must be at least 1");
        }
        if width > self.width {
            bail!(
                "placement width resolved to {width}, exceeds screen width {}",
                self.width
            );
        }
        if height > self.height {
            bail!(
                "placement height resolved to {height}, exceeds screen height {}",
                self.height
            );
        }

        Ok(())
    }

    pub fn resolve_metric(metric: &PlacementMetric, axis: i32) -> i32 {
        match metric {
            PlacementMetric::Percent(pct) => {
                let resolved = i64::from(axis) * i64::from(*pct) / 100;
                i32::try_from(resolved).expect("screen metric should fit within i32")
            }
            PlacementMetric::Pixels(px) => *px,
        }
    }

    fn place_axis(
        screen_start: i32,
        screen_length: i32,
        length: i32,
        base_start: i32,
        offset: i32,
    ) -> (i32, i32) {
        let screen_end = screen_start + screen_length;
        let desired_start = base_start + offset;
        let desired_end = desired_start + length;

        let clipped_start = desired_start.max(screen_start);
        let clipped_end = desired_end.min(screen_end);
        if length == screen_length && clipped_start < clipped_end {
            return (clipped_start, clipped_end - clipped_start);
        }

        let max_start = screen_end - length;
        (desired_start.clamp(screen_start, max_start), length)
    }
}

pub fn hidden_rect_for_screens(screens: &[ScreenInfo], visible: &FrameGeometry) -> FrameGeometry {
    let top = screens
        .iter()
        .map(|screen| screen.y)
        .min()
        .unwrap_or(visible.y);

    FrameGeometry {
        x: visible.x,
        y: top - visible.height,
        width: visible.width,
        height: visible.height,
    }
}

pub fn parse_support_information(text: &str) -> Result<Vec<ScreenInfo>> {
    let mut screens = Vec::new();

    for header in screen_header_re().find_iter(text) {
        let section = &text[header.start()..];
        let index_caps = screen_header_re()
            .captures(header.as_str())
            .context("failed to parse screen header")?;
        let index = index_caps[1].parse::<u32>()?;
        let name = screen_name_re()
            .captures(section)
            .map_or_else(|| format!("Screen {index}"), |caps| caps[1].to_string());
        let geometry = screen_geometry_re()
            .captures(section)
            .with_context(|| format!("screen {index} is missing geometry"))?;

        screens.push(ScreenInfo {
            index,
            name,
            x: geometry[1].parse()?,
            y: geometry[2].parse()?,
            width: geometry[3].parse()?,
            height: geometry[4].parse()?,
        });
    }

    if screens.is_empty() {
        bail!("no screens found in KWin support information");
    }

    Ok(screens)
}

#[cfg(test)]
mod tests {
    use super::{ScreenInfo, hidden_rect_for_screens, parse_support_information};
    use crate::config::{PlacementConfig, PlacementMetric, PlacementPosition};
    use crate::wm::FrameGeometry;

    #[test]
    fn parses_support_information_happy_path() {
        let text = "\
Screen 0:
Name: eDP-1
Geometry: 0,0,1920x1080

Screen 1:
Name: HDMI-1
Geometry: 1920,0,2560x1440
";

        let screens = parse_support_information(text).unwrap();
        assert_eq!(screens.len(), 2);
        assert_eq!(screens[0].name, "eDP-1");
        assert_eq!(screens[1].x, 1920);
        assert_eq!(screens[1].height, 1440);
    }

    #[test]
    fn rejects_screen_without_geometry() {
        let text = "\
Screen 0:
Name: eDP-1
";

        let error = parse_support_information(text).unwrap_err();
        assert!(error.to_string().contains("missing geometry"));
    }

    #[test]
    fn rejects_when_no_screens_exist() {
        let error = parse_support_information("Name: eDP-1").unwrap_err();
        assert!(error.to_string().contains("no screens found"));
    }

    #[test]
    fn parses_negative_screen_coordinates() {
        let text = "\
Screen 0:
Name: HDMI-1
Geometry: -1920,-1080,1920x1080
";

        let screens = parse_support_information(text).unwrap();
        assert_eq!((screens[0].x, screens[0].y), (-1920, -1080));
    }

    fn screen() -> ScreenInfo {
        ScreenInfo {
            index: 0,
            name: "eDP-1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn computes_full_screen_default_rect() {
        let rect = screen().placement_rect(&PlacementConfig::default());
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0, 0, 1920, 1080)
        );
    }

    #[test]
    fn computes_right_half_rect() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(50),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::Right,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (960, 0, 960, 1080)
        );
    }

    #[test]
    fn computes_fixed_pixel_width_and_percent_height() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Pixels(1280),
            height: PlacementMetric::Percent(70),
            position: PlacementPosition::TopRight,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (640, 0, 1280, 756)
        );
    }

    #[test]
    fn computes_bottom_half_rect() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(100),
            height: PlacementMetric::Percent(50),
            position: PlacementPosition::Bottom,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0, 540, 1920, 540)
        );
    }

    #[test]
    fn computes_centered_sixty_by_seventy_rect() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(60),
            height: PlacementMetric::Percent(70),
            position: PlacementPosition::Center,
            offset_x: PlacementMetric::Pixels(0),
            offset_y: PlacementMetric::Pixels(0),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (384, 162, 1152, 756)
        );
    }

    #[test]
    fn applies_pixel_offsets() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(60),
            height: PlacementMetric::Percent(70),
            position: PlacementPosition::Center,
            offset_x: PlacementMetric::Pixels(24),
            offset_y: PlacementMetric::Pixels(12),
        });
        assert_eq!((rect.x, rect.y), (408, 174));
    }

    #[test]
    fn clips_full_size_rect_after_positive_offsets() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(100),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::TopLeft,
            offset_x: PlacementMetric::Pixels(20),
            offset_y: PlacementMetric::Pixels(12),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (20, 12, 1900, 1068)
        );
    }

    #[test]
    fn clips_full_size_rect_after_negative_offsets() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(100),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::BottomRight,
            offset_x: PlacementMetric::Pixels(-20),
            offset_y: PlacementMetric::Pixels(-12),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0, 0, 1900, 1068)
        );
    }

    #[test]
    fn clips_full_width_without_shrinking_partial_height() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(100),
            height: PlacementMetric::Percent(50),
            position: PlacementPosition::TopLeft,
            offset_x: PlacementMetric::Pixels(20),
            offset_y: PlacementMetric::Pixels(12),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (20, 12, 1900, 540)
        );
    }

    #[test]
    fn clips_full_height_without_shrinking_partial_width() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(50),
            height: PlacementMetric::Percent(100),
            position: PlacementPosition::TopLeft,
            offset_x: PlacementMetric::Pixels(20),
            offset_y: PlacementMetric::Pixels(12),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (20, 12, 960, 1068)
        );
    }

    #[test]
    fn applies_percent_offsets() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(60),
            height: PlacementMetric::Percent(70),
            position: PlacementPosition::Center,
            offset_x: PlacementMetric::Percent(-2),
            offset_y: PlacementMetric::Percent(-2),
        });
        assert_eq!((rect.x, rect.y), (346, 141));
    }

    #[test]
    fn clamps_offsets_at_screen_bounds() {
        let rect = screen().placement_rect(&PlacementConfig {
            width: PlacementMetric::Percent(50),
            height: PlacementMetric::Percent(50),
            position: PlacementPosition::TopLeft,
            offset_x: PlacementMetric::Pixels(2000),
            offset_y: PlacementMetric::Pixels(-100),
        });
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (960, 0, 960, 540)
        );
    }

    #[test]
    fn validate_placement_rejects_pixel_width_exceeding_screen() {
        let err = screen()
            .validate_placement(&PlacementConfig {
                width: PlacementMetric::Pixels(4000),
                height: PlacementMetric::Percent(100),
                position: PlacementPosition::TopLeft,
                offset_x: PlacementMetric::Pixels(0),
                offset_y: PlacementMetric::Pixels(0),
            })
            .unwrap_err();
        assert!(err.to_string().contains("exceeds screen width"));
    }

    #[test]
    fn validate_placement_rejects_pixel_height_exceeding_screen() {
        let err = screen()
            .validate_placement(&PlacementConfig {
                width: PlacementMetric::Percent(100),
                height: PlacementMetric::Pixels(4000),
                position: PlacementPosition::TopLeft,
                offset_x: PlacementMetric::Pixels(0),
                offset_y: PlacementMetric::Pixels(0),
            })
            .unwrap_err();
        assert!(err.to_string().contains("exceeds screen height"));
    }

    #[test]
    fn validate_placement_rejects_percent_resolving_below_one() {
        let tiny_screen = ScreenInfo {
            index: 0,
            name: "tiny".into(),
            x: 0,
            y: 0,
            width: 50,
            height: 50,
        };
        let err = tiny_screen
            .validate_placement(&PlacementConfig {
                width: PlacementMetric::Percent(1),
                height: PlacementMetric::Percent(100),
                position: PlacementPosition::TopLeft,
                offset_x: PlacementMetric::Pixels(0),
                offset_y: PlacementMetric::Pixels(0),
            })
            .unwrap_err();
        assert!(err.to_string().contains("must be at least 1"));
    }

    #[test]
    fn validate_placement_accepts_full_screen_default() {
        screen()
            .validate_placement(&PlacementConfig::default())
            .unwrap();
    }

    #[test]
    fn computes_hidden_rect_above_virtual_desktop() {
        let screens = vec![
            screen(),
            ScreenInfo {
                index: 1,
                name: "top".into(),
                x: 0,
                y: -1080,
                width: 1920,
                height: 1080,
            },
        ];
        let visible = FrameGeometry {
            x: 640,
            y: 0,
            width: 1280,
            height: 720,
        };

        let hidden = hidden_rect_for_screens(&screens, &visible);
        assert_eq!(
            (hidden.x, hidden.y, hidden.width, hidden.height),
            (640, -1800, 1280, 720)
        );
    }
}
