use super::Rgb;
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::{GetDC, GetPixel, ReleaseDC},
    UI::{
        Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture},
        WindowsAndMessaging::GetCursorPos,
    },
};

/// Keep receiving the user's pointer click even when it lands outside the picker window.
pub(super) struct Capture(HWND);

impl Capture {
    pub fn new(window: &Window) -> anyhow::Result<Self> {
        let handle = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow::anyhow!("Screen sampling needs a window: {error}"))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            anyhow::bail!("Screen sampling needs a Windows window.");
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        // Called on the window's own event thread, after the eyedropper click.
        unsafe { SetCapture(hwnd) };
        anyhow::ensure!(
            unsafe { GetCapture() } == hwnd,
            "The window could not capture the screen click."
        );
        Ok(Self(hwnd))
    }

    pub fn sample(&self) -> anyhow::Result<Rgb> {
        let mut point = POINT::default();
        // Read only the screen pixel the user clicked; don't copy the desktop into memory.
        unsafe { GetCursorPos(&mut point) }?;
        let screen = unsafe { GetDC(None) };
        anyhow::ensure!(
            !screen.0.is_null(),
            "Windows did not provide a screen device context."
        );
        let color = unsafe { GetPixel(screen, point.x, point.y) }.0;
        unsafe { ReleaseDC(None, screen) };
        decode_colorref(color)
            .ok_or_else(|| anyhow::anyhow!("Windows could not sample this screen pixel."))
    }
}

fn decode_colorref(value: u32) -> Option<Rgb> {
    let [red, green, blue, _] = value.to_le_bytes();
    (value != u32::MAX).then_some([red, green, blue])
}

#[cfg(test)]
#[test]
fn decodes_gdi_channel_order_without_treating_failure_as_white() {
    assert_eq!(decode_colorref(0x0056_3412), Some([0x12, 0x34, 0x56]));
    assert_eq!(decode_colorref(0x00ff_ffff), Some([255; 3]));
    assert_eq!(decode_colorref(u32::MAX), None);
}

impl Drop for Capture {
    fn drop(&mut self) {
        if unsafe { GetCapture() } == self.0 {
            let _ = unsafe { ReleaseCapture() };
        }
    }
}
