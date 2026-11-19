//! OS screen sampling behind the shared GPUI picker. No screen image is retained.
use super::*;

#[cfg(windows)]
#[path = "settings_picker_sampler_windows.rs"]
mod windows;

pub(super) struct Sampling {
    #[cfg(windows)]
    capture: windows::Capture,
}

impl Picker {
    pub(super) fn begin_sampling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sampling.is_some() {
            return;
        }
        self.sampler_focus.focus(window);
        #[cfg(windows)]
        {
            match windows::Capture::new(window) {
                Ok(capture) => self.sampling = Some(Sampling { capture }),
                Err(error) => cx.emit(Event::SamplingError(error.to_string())),
            }
            cx.notify();
        }
        #[cfg(not(windows))]
        {
            let task = start(cx);
            self.sampling = Some(Sampling {});
            self.sampling_epoch = self.sampling_epoch.wrapping_add(1);
            let epoch = self.sampling_epoch;
            cx.spawn(async move |picker, cx| {
                let result = task.await;
                let _ = picker.update(cx, |picker, cx| {
                    if picker.sampling_epoch == epoch && picker.sampling.is_some() {
                        picker.apply_sample(result, cx);
                    }
                });
            })
            .detach();
            cx.notify();
        }
    }

    pub(super) fn sample_on_click(&mut self, button: gpui::MouseButton, cx: &mut Context<Self>) {
        #[cfg(windows)]
        if let Some(Sampling { capture }) = self.sampling.take() {
            let result = if button == gpui::MouseButton::Left {
                capture.sample().map(Some)
            } else {
                Ok(None)
            };
            drop(capture);
            self.apply_sample(result, cx);
        }
        #[cfg(not(windows))]
        let _ = (button, cx);
    }

    fn apply_sample(&mut self, result: anyhow::Result<Option<Rgb>>, cx: &mut Context<Self>) {
        self.sampling = None;
        match result {
            Ok(Some(rgb)) => {
                self.move_to_color(rgb);
                self.select(rgb, cx);
            }
            Ok(None) => {}
            Err(error) => cx.emit(Event::SamplingError(error.to_string())),
        }
        cx.notify();
    }
}

#[cfg(not(windows))]
fn to_rgb(values: [f64; 3]) -> anyhow::Result<Rgb> {
    anyhow::ensure!(
        values.iter().all(|value| (0. ..=1.).contains(value)),
        "Screen color is out of range."
    );
    Ok(values.map(|value| (value * 255.).round() as u8))
}

#[cfg(target_os = "linux")]
fn start(cx: &Context<Picker>) -> gpui::Task<anyhow::Result<Option<Rgb>>> {
    cx.background_executor().spawn(async {
        use ashpd::{
            Error,
            desktop::{Color, ResponseError},
        };
        let picked = async { Color::pick().send().await?.response() }.await;
        match picked {
            Ok(color) => to_rgb([color.red(), color.green(), color.blue()]).map(Some),
            Err(Error::Response(ResponseError::Cancelled)) => Ok(None),
            Err(_) => anyhow::bail!("The desktop portal could not sample a color."),
        }
    })
}

#[cfg(target_os = "macos")]
fn start(cx: &mut Context<Picker>) -> gpui::Task<anyhow::Result<Option<Rgb>>> {
    use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};
    let (send, receive) = tokio::sync::oneshot::channel();
    let send = Cell::new(Some(send));
    let handler = block2::RcBlock::new(move |ptr: *mut NSColor| {
        let Some(send) = send.take() else { return };
        // The sampler calls back on the main thread, with a borrowed NSColor or null on cancel.
        let sample = unsafe { ptr.as_ref() }
            .map(|color| {
                let color = color
                    .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                    .ok_or_else(|| anyhow::anyhow!("Screen color cannot be converted to sRGB."))?;
                to_rgb([
                    color.redComponent(),
                    color.greenComponent(),
                    color.blueComponent(),
                ])
            })
            .transpose();
        let _ = send.send(sample);
    });
    // AppKit retains both the sampler and handler for the duration of the sampling session.
    unsafe { NSColorSampler::new().showSamplerWithSelectionHandler(&handler) };
    cx.spawn(async move |_, _| receive.await?)
}
