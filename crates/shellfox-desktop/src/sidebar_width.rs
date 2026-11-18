//! Desired sidebar width is persisted only by user resize, not viewport layout.
pub(crate) fn clamp_width(width: f32, window: f32) -> f32 {
    let width = if width.is_finite() { width } else { 220. };
    if !window.is_finite() || window <= 0. {
        return width.max(160.);
    }
    width.max(160.).min((window / 2.).max(160.))
}

#[cfg(test)]
mod tests {
    use super::*;
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
