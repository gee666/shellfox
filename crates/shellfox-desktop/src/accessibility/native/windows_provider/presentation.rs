use super::{SemanticError, WindowOwner};
use gpui::{Bounds, DevicePixels, Pixels, Size};
use windows::Win32::{
    Foundation::RECT,
    UI::WindowsAndMessaging::{
        GetClientRect, GetForegroundWindow, GetWindowRect, IsWindowVisible, SWP_NOACTIVATE,
        SWP_NOOWNERZORDER, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos,
    },
};

/// Uses the same client/nonclient contract as published GPUI 0.2.2, with measured extents.
/// It never reads or changes GPUI's private initial-placement state.
fn outer_rect(
    bounds: Bounds<Pixels>,
    scale: f32,
    outer: RECT,
    client: RECT,
) -> Result<RECT, SemanticError> {
    let invalid = || SemanticError::Native("invalid hidden-window geometry".into());
    if !scale.is_finite() || scale <= 0. {
        return Err(invalid());
    }
    for value in [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ] {
        let physical = f32::from(value) * scale;
        if !physical.is_finite()
            || physical.round() < i32::MIN as f32
            || physical.round() >= i32::MAX as f32
        {
            return Err(invalid());
        }
    }
    let width_delta = (i64::from(outer.right) - i64::from(outer.left))
        - (i64::from(client.right) - i64::from(client.left));
    let height_delta = (i64::from(outer.bottom) - i64::from(outer.top))
        - (i64::from(client.bottom) - i64::from(client.top));
    let pixels = bounds.to_device_pixels(scale);
    if pixels.size.width.0 <= 0 || pixels.size.height.0 <= 0 || width_delta < 0 || height_delta < 0
    {
        return Err(invalid());
    }
    let left = i64::from(pixels.origin.x.0) - width_delta / 2;
    let top = i64::from(pixels.origin.y.0) - height_delta / 2;
    let right = left + i64::from(pixels.size.width.0) + width_delta;
    let bottom = top + i64::from(pixels.size.height.0) + height_delta;
    let convert = |value| i32::try_from(value).map_err(|_| invalid());
    let rect = RECT {
        left: convert(left)?,
        top: convert(top)?,
        right: convert(right)?,
        bottom: convert(bottom)?,
    };
    rect.right
        .checked_sub(rect.left)
        .filter(|width| *width > 0)
        .ok_or_else(invalid)?;
    rect.bottom
        .checked_sub(rect.top)
        .filter(|height| *height > 0)
        .ok_or_else(invalid)?;
    Ok(rect)
}

pub(super) fn show(
    owner: &WindowOwner,
    bounds: Bounds<Pixels>,
    scale: f32,
) -> Result<Size<DevicePixels>, SemanticError> {
    owner.validate_hidden_native()?;
    let foreground = unsafe { GetForegroundWindow() };
    let mut outer = RECT::default();
    let mut client = RECT::default();
    // SAFETY: A live provider's cookie and same-thread checks identify this owned HWND.
    // No GPUI Window/App borrow is held here, so synchronous resize/paint callbacks can update GPUI.
    unsafe {
        GetWindowRect(owner.hwnd, &mut outer).map_err(|e| SemanticError::Native(e.to_string()))?;
        GetClientRect(owner.hwnd, &mut client).map_err(|e| SemanticError::Native(e.to_string()))?;
    }
    let rect = outer_rect(bounds, scale, outer, client)?;
    let expected_client = bounds.to_device_pixels(scale).size;
    owner.validate_hidden_native()?;
    unsafe {
        SetWindowPos(
            owner.hwnd,
            None,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_SHOWWINDOW | SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOOWNERZORDER,
        )
    }
    .map_err(|e| SemanticError::Native(e.to_string()))?;
    owner.validate_native()?;
    let mut shown_client = RECT::default();
    let mut shown_outer = RECT::default();
    unsafe {
        GetClientRect(owner.hwnd, &mut shown_client)
            .map_err(|e| SemanticError::Native(e.to_string()))?;
        GetWindowRect(owner.hwnd, &mut shown_outer)
            .map_err(|e| SemanticError::Native(e.to_string()))?;
    }
    if [
        shown_outer.left,
        shown_outer.top,
        shown_outer.right,
        shown_outer.bottom,
    ] != [rect.left, rect.top, rect.right, rect.bottom]
    {
        return Err(SemanticError::Native(
            "native outer bounds differ from the presentation plan".into(),
        ));
    }
    if shown_client.right.checked_sub(shown_client.left) != Some(expected_client.width.0)
        || shown_client.bottom.checked_sub(shown_client.top) != Some(expected_client.height.0)
    {
        return Err(SemanticError::Native(
            "native client bounds differ from the presentation plan".into(),
        ));
    }
    if !unsafe { IsWindowVisible(owner.hwnd) }.as_bool()
        || unsafe { GetForegroundWindow() } != foreground
    {
        // Do not try to restore another window's focus or synthesize focus messages.
        return Err(SemanticError::Native(
            "nonactivating presentation did not preserve visibility/foreground".into(),
        ));
    }
    Ok(expected_client)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds(x: f32) -> Bounds<Pixels> {
        gpui::bounds(
            gpui::point(gpui::px(x), gpui::px(30.)),
            gpui::size(gpui::px(1200.), gpui::px(760.)),
        )
    }
    #[test]
    fn measured_native_border_extents_preserve_requested_client_geometry() {
        let outer = RECT {
            left: 0,
            top: 0,
            right: 1216,
            bottom: 776,
        };
        let client = RECT {
            left: 0,
            top: 0,
            right: 1200,
            bottom: 760,
        };
        let rect = outer_rect(bounds(20.), 1., outer, client).unwrap();
        assert_eq!(
            (rect.left, rect.top, rect.right, rect.bottom),
            (12, 22, 1228, 798)
        );
    }
    #[test]
    fn negative_monitor_coordinates_and_dpi_are_not_guessed_or_clamped() {
        let outer = RECT {
            left: 0,
            top: 0,
            right: 1817,
            bottom: 1157,
        };
        let client = RECT {
            left: 0,
            top: 0,
            right: 1800,
            bottom: 1140,
        };
        let rect = outer_rect(bounds(-100.), 1.5, outer, client).unwrap();
        assert_eq!((rect.left, rect.top), (-158, 37));
        assert_eq!(
            (rect.right - rect.left, rect.bottom - rect.top),
            (1817, 1157)
        );
    }
    #[test]
    fn invalid_geometry_is_an_error_before_any_show_operation() {
        assert!(outer_rect(bounds(0.), f32::NAN, RECT::default(), RECT::default()).is_err());
        assert!(outer_rect(bounds(f32::MAX), 2., RECT::default(), RECT::default()).is_err());
        let client = RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        assert!(outer_rect(bounds(0.), 1., RECT::default(), client).is_err());
    }
}
