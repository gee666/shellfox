//! Desired sidebar width is persisted only by user resize, not viewport layout.
pub(crate) fn clamp_width(width: f32, window: f32) -> f32 {
    let width = if width.is_finite() { width } else { 220. };
    if !window.is_finite() || window <= 0. {
        return width.max(160.);
    }
    width.max(160.).min((window / 2.).max(160.))
}

#[derive(Clone, Copy)]
pub(crate) struct SidebarDrag {
    start: f32,
    width: f32,
}
impl SidebarDrag {
    pub(crate) fn start(pointer: f32, width: f32, viewport: f32) -> Self {
        Self {
            start: pointer,
            width: clamp_width(width, viewport),
        }
    }
    pub(crate) fn width(self, pointer: f32, viewport: f32) -> f32 {
        clamp_width(self.width + pointer - self.start, viewport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dragging_keeps_the_mouse_offset_inside_the_handle() {
        let drag = SidebarDrag::start(223., 220., 1200.);
        assert_eq!(drag.width(223., 1200.), 220.);
        assert_eq!(drag.width(263., 1200.), 260.);
        assert_eq!(drag.width(1200., 1200.), 600.);
    }

    #[test]
    fn resizer_matches_existing_renderer_bounds() {
        assert_eq!(clamp_width(220., 1220.), 220.);
        assert_eq!(clamp_width(10., 1220.), 160.);
        assert_eq!(clamp_width(900., 850.), 425.);
        assert_eq!(clamp_width(f32::NAN, 1220.), 220.);
        // Render clamps are derived, not written into the user's preferred width.
        let preferred = 220.;
        let displayed: Vec<_> = [0., f32::NAN, f32::INFINITY, 300., 400., 1216.]
            .map(|viewport| clamp_width(preferred, viewport))
            .into();
        assert_eq!(displayed, [220., 220., 220., 160., 200., 220.]);
        assert_eq!(clamp_width(clamp_width(900., 850.), 1216.), 425.); // actual user drag persists
        assert_eq!(clamp_width(f32::INFINITY, 1216.), 220.);
    }
}
