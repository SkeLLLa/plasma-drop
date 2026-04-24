use crate::config::{AnimationConfig, AnimationEasing, AnimationStyle};
use crate::screen::ScreenInfo;
use crate::wm::FrameGeometry;

const FRAME_INTERVAL_MS: u16 = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowState {
    pub geometry: Option<FrameGeometry>,
    pub opacity: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionPhase {
    Show,
    Hide,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionPlan {
    duration_ms: u16,
    easing: AnimationEasing,
    setup: WindowState,
    keyframes: Vec<KeyframeTrack>,
    teardown: WindowState,
}

#[derive(Debug, Clone, PartialEq)]
enum KeyframeTrack {
    Geometry {
        from: FrameGeometry,
        to: FrameGeometry,
    },
    Opacity {
        from: f64,
        to: f64,
    },
}

impl TransitionPlan {
    pub fn from_config(
        config: &AnimationConfig,
        screen: &ScreenInfo,
        visible: &FrameGeometry,
        phase: TransitionPhase,
    ) -> Option<Self> {
        if matches!(config.style, AnimationStyle::None) || config.duration_ms == 0 {
            return None;
        }

        let hidden = screen.hidden_rect_for(visible);
        let mut setup = WindowState {
            geometry: None,
            opacity: None,
        };
        let mut keyframes = Vec::new();
        let mut teardown = WindowState {
            geometry: None,
            opacity: None,
        };

        match phase {
            TransitionPhase::Show => {
                if uses_geometry_track(config.style) {
                    setup.geometry = Some(hidden.clone());
                    keyframes.push(KeyframeTrack::Geometry {
                        from: hidden,
                        to: visible.clone(),
                    });
                } else {
                    setup.geometry = Some(visible.clone());
                }

                if uses_opacity_track(config.style) {
                    setup.opacity = Some(0.0);
                    keyframes.push(KeyframeTrack::Opacity { from: 0.0, to: 1.0 });
                }
            }
            TransitionPhase::Hide => {
                setup.geometry = Some(visible.clone());

                if uses_geometry_track(config.style) {
                    keyframes.push(KeyframeTrack::Geometry {
                        from: visible.clone(),
                        to: hidden.clone(),
                    });
                }

                if uses_opacity_track(config.style) {
                    setup.opacity = Some(1.0);
                    keyframes.push(KeyframeTrack::Opacity { from: 1.0, to: 0.0 });
                    teardown.opacity = Some(1.0);
                }

                if !uses_geometry_track(config.style) {
                    teardown.geometry = Some(hidden);
                }
            }
        }

        Some(Self {
            duration_ms: config.duration_ms,
            easing: config.easing,
            setup,
            keyframes,
            teardown,
        })
    }

    pub fn setup_state(&self) -> WindowState {
        self.setup.clone()
    }

    pub fn teardown_state(&self) -> WindowState {
        self.teardown.clone()
    }

    pub fn frame_count(&self) -> u16 {
        self.duration_ms.div_ceil(FRAME_INTERVAL_MS).max(1)
    }

    pub fn frame_state(&self, frame_index: u16) -> WindowState {
        let progress = f64::from(frame_index) / f64::from(self.frame_count());
        let eased = ease(progress, self.easing);
        let mut state = WindowState {
            geometry: None,
            opacity: None,
        };

        for track in &self.keyframes {
            match track {
                KeyframeTrack::Geometry { from, to } => {
                    state.geometry = Some(interpolate_geometry(from, to, eased));
                }
                KeyframeTrack::Opacity { from, to } => {
                    state.opacity = Some(interpolate_scalar(*from, *to, eased));
                }
            }
        }

        state
    }

    pub fn final_state(&self) -> WindowState {
        self.frame_state(self.frame_count())
    }
}

const fn uses_geometry_track(style: AnimationStyle) -> bool {
    matches!(style, AnimationStyle::Slide | AnimationStyle::SlideFade)
}

const fn uses_opacity_track(style: AnimationStyle) -> bool {
    matches!(style, AnimationStyle::Fade | AnimationStyle::SlideFade)
}

fn interpolate_geometry(from: &FrameGeometry, to: &FrameGeometry, progress: f64) -> FrameGeometry {
    FrameGeometry {
        x: interpolate_i32(from.x, to.x, progress),
        y: interpolate_i32(from.y, to.y, progress),
        width: interpolate_i32(from.width, to.width, progress),
        height: interpolate_i32(from.height, to.height, progress),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn interpolate_i32(from: i32, to: i32, progress: f64) -> i32 {
    let delta = f64::from(to - from);
    delta.mul_add(progress, f64::from(from)).round() as i32
}

fn interpolate_scalar(from: f64, to: f64, progress: f64) -> f64 {
    (to - from).mul_add(progress, from)
}

fn ease(progress: f64, easing: AnimationEasing) -> f64 {
    match easing {
        AnimationEasing::Linear => progress,
        AnimationEasing::EaseOut => 1.0 - (1.0 - progress).powi(3),
        AnimationEasing::EaseInOut => {
            if progress < 0.5 {
                4.0 * progress.powi(3)
            } else {
                1.0 - (-2.0f64).mul_add(progress, 2.0).powi(3) / 2.0
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TransitionPhase, TransitionPlan};
    use crate::config::{AnimationConfig, AnimationEasing, AnimationStyle};
    use crate::screen::ScreenInfo;
    use crate::wm::FrameGeometry;

    fn screen() -> ScreenInfo {
        ScreenInfo {
            index: 0,
            name: "screen".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    const fn visible() -> FrameGeometry {
        FrameGeometry {
            x: 960,
            y: 0,
            width: 960,
            height: 1080,
        }
    }

    #[test]
    fn slide_show_animates_geometry_only() {
        let plan = TransitionPlan::from_config(
            &AnimationConfig {
                style: AnimationStyle::Slide,
                easing: AnimationEasing::EaseOut,
                duration_ms: 160,
            },
            &screen(),
            &visible(),
            TransitionPhase::Show,
        )
        .unwrap();

        assert_eq!(plan.setup_state().geometry.unwrap().y, -1080);
        assert_eq!(plan.setup_state().opacity, None);
        assert_eq!(plan.final_state().geometry.unwrap(), visible());
    }

    #[test]
    fn fade_hide_restores_opacity_after_animation() {
        let plan = TransitionPlan::from_config(
            &AnimationConfig {
                style: AnimationStyle::Fade,
                easing: AnimationEasing::EaseInOut,
                duration_ms: 160,
            },
            &screen(),
            &visible(),
            TransitionPhase::Hide,
        )
        .unwrap();

        assert_eq!(plan.setup_state().geometry.unwrap(), visible());
        assert_eq!(plan.final_state().opacity, Some(0.0));
        assert_eq!(plan.teardown_state().opacity, Some(1.0));
        assert_eq!(plan.teardown_state().geometry.unwrap().y, -1080);
    }

    #[test]
    fn disabled_animation_returns_none() {
        let plan = TransitionPlan::from_config(
            &AnimationConfig {
                style: AnimationStyle::None,
                easing: AnimationEasing::Linear,
                duration_ms: 1000,
            },
            &screen(),
            &visible(),
            TransitionPhase::Show,
        );

        assert!(plan.is_none());
    }
}
