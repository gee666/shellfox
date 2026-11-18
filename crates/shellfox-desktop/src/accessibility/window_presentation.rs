use super::SemanticError;
use gpui::{Bounds, Pixels, WindowBounds, WindowOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowPresentation {
    Activate,
    WithoutActivation,
}

/// Preserve creation geometry while attaching the native provider to a hidden window.
/// This does not create or show a window. Root owns the returned options and plan.
pub struct HiddenWindowPresentation {
    pub(super) mode: WindowPresentation,
    pub(super) bounds: Option<Bounds<Pixels>>,
}

impl HiddenWindowPresentation {
    pub fn prepare(
        mut options: WindowOptions,
        mode: WindowPresentation,
    ) -> Result<(WindowOptions, Self), SemanticError> {
        if cfg!(target_os = "macos") && options.tabbing_identifier.is_some() {
            return Err(SemanticError::Native(
                "automatic macOS tabbing can show before accessibility attaches".into(),
            ));
        }
        let bounds = if mode == WindowPresentation::WithoutActivation {
            if !cfg!(target_os = "windows") {
                return Err(SemanticError::Native(format!(
                    "nonactivating acceptance presentation is not implemented on {}",
                    std::env::consts::OS
                )));
            }
            let Some(WindowBounds::Windowed(bounds)) = options.window_bounds else {
                return Err(SemanticError::Native(
                    "nonactivating presentation requires explicit Windowed creation bounds".into(),
                ));
            };
            if ![bounds.left(), bounds.top(), bounds.right(), bounds.bottom()]
                .into_iter()
                .all(|value| f32::from(value).is_finite())
                || bounds.size.width <= gpui::px(0.)
                || bounds.size.height <= gpui::px(0.)
            {
                return Err(SemanticError::Native("invalid hidden-window bounds".into()));
            }
            Some(bounds)
        } else {
            None
        };
        options.show = false;
        options.focus = false;
        Ok((options, Self { mode, bounds }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_plan_is_hidden_unfocused_without_changing_creation_style() {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui::bounds(
                gpui::point(gpui::px(20.), gpui::px(30.)),
                gpui::size(gpui::px(1200.), gpui::px(760.)),
            ))),
            ..Default::default()
        };
        let old_bounds = options.window_bounds;
        let old_resizable = options.is_resizable;
        let (options, plan) =
            HiddenWindowPresentation::prepare(options, WindowPresentation::Activate).unwrap();
        assert!(!options.show && !options.focus);
        assert_eq!(options.window_bounds, old_bounds);
        assert_eq!(options.is_resizable, old_resizable);
        assert_eq!(plan.mode, WindowPresentation::Activate);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn acceptance_plan_preserves_explicit_bounds_and_rejects_guessed_placement() {
        assert!(
            HiddenWindowPresentation::prepare(
                WindowOptions::default(),
                WindowPresentation::WithoutActivation
            )
            .is_err()
        );
        let bounds = gpui::bounds(
            gpui::point(gpui::px(-100.), gpui::px(40.)),
            gpui::size(gpui::px(1200.), gpui::px(760.)),
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        let (_, plan) =
            HiddenWindowPresentation::prepare(options, WindowPresentation::WithoutActivation)
                .unwrap();
        assert_eq!(plan.bounds, Some(bounds));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unsupported_acceptance_plan_never_falls_back_to_activation() {
        assert!(
            HiddenWindowPresentation::prepare(
                WindowOptions::default(),
                WindowPresentation::WithoutActivation
            )
            .is_err()
        );
    }
}
