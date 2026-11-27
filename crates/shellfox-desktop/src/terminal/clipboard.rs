//! Copy targets. Linux also has the PRIMARY selection: select to copy, middle click to
//! paste, as xterm.js did in 0.9. Windows and macOS have only the clipboard.
use super::TerminalView;
use gpui::{ClipboardItem, Context};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CopyTarget {
    Clipboard,
    #[cfg(target_os = "linux")]
    Primary,
}

impl CopyTarget {
    pub(super) fn write(self, text: String, cx: &gpui::App) {
        let item = ClipboardItem::new_string(text);
        match self {
            Self::Clipboard => cx.write_to_clipboard(item),
            #[cfg(target_os = "linux")]
            Self::Primary => cx.write_to_primary(item),
        }
    }
}

impl TerminalView {
    /// Middle click: paste the PRIMARY selection, which may come from another app.
    #[cfg(target_os = "linux")]
    pub(super) fn paste_primary(&mut self, cx: &mut Context<Self>) {
        self.paste_text(cx.read_from_primary().and_then(|item| item.text()));
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn paste_primary(&mut self, _: &mut Context<Self>) {}

    /// A finished mouse selection becomes the PRIMARY selection.
    #[cfg(target_os = "linux")]
    pub(super) fn publish_primary(&mut self, cx: &mut Context<Self>) {
        self.copy_selection_to(CopyTarget::Primary, cx);
    }

    #[cfg(not(target_os = "linux"))]
    pub(super) fn publish_primary(&mut self, _: &mut Context<Self>) {}
}
