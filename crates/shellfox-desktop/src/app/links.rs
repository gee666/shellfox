use super::{
    link_gesture::FilePress,
    link_ranges::{self, LinkRange},
    runtime::Pending,
    terminal_file_link::{self, TerminalFileLink},
    Backend, TerminalView,
};
use gpui::{ClipboardItem, Context};
use shellfox_ghostty_vt::ViewportPoint;
use shellfox_runtime::Command;
use url::Url;

const MAX_LINK_BYTES: usize = 2048;

#[derive(Default)]
pub(super) struct LinkState {
    pub hovered: Option<LinkRange>,
    pub menu: Option<super::link_menu::LinkMenu>,
    pub menu_press: Option<super::link_menu::LinkMenuPress>,
    pub file_press: Option<FilePress>,
    pub(super) files_enabled: bool,
    point: Option<HoverPoint>,
    pending: bool,
}

impl gpui::EventEmitter<TerminalFileLink> for TerminalView {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HoverPoint {
    point: ViewportPoint,
    content: (u64, u64, u16),
}

impl TerminalView {
    fn link_point(&self, point: ViewportPoint) -> HoverPoint {
        let (sequence, top) = match &self.screen {
            super::SharedScreen::Native(frame) => (frame.sequence, frame.top_row),
            super::SharedScreen::Peer(frame) => (frame.sequence, frame.rendered.top_row),
            _ => (0, 0),
        };
        let content = (sequence, top, self.screen.cols);
        HoverPoint { point, content }
    }

    fn osc_link_at(&self, point: ViewportPoint) -> bool {
        let idx = |x| usize::from(self.screen.cols) * usize::from(point.y) + usize::from(x);
        self.screen.cells.get(idx(point.x)).is_some_and(|cell| {
            cell.has_hyperlink
                || cell.width == 0
                    && point.x > 0
                    && self
                        .screen
                        .cells
                        .get(idx(point.x - 1))
                        .is_some_and(|cell| cell.has_hyperlink)
        })
    }

    fn plain_link_at(&self, point: ViewportPoint) -> Option<LinkRange> {
        let metadata = match &self.screen {
            super::SharedScreen::Native(frame) => &frame.row_metadata[..],
            super::SharedScreen::Peer(frame) => &frame.rendered.row_metadata[..],
            _ => &[],
        };
        link_ranges::link_at(&self.screen, metadata, point, self.links.files_enabled)
    }

    pub(super) fn visible_link_at(&self, point: ViewportPoint) -> Option<LinkRange> {
        if self.osc_link_at(point) {
            if self.links.point == Some(self.link_point(point)) {
                self.links.hovered.clone()
            } else {
                None
            }
        } else {
            self.plain_link_at(point)
        }
    }

    pub(super) fn update_link_hover(
        &mut self,
        point: Option<ViewportPoint>,
        cx: &mut Context<Self>,
    ) {
        let point = point.map(|point| self.link_point(point));
        if self.links.point == point {
            return;
        }
        self.links.point = point;
        let had_link = self.links.hovered.take().is_some();
        self.probe_link_hover(cx);
        if had_link || self.links.hovered.is_some() {
            cx.notify();
        }
    }

    fn probe_link_hover(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.links.point else {
            return;
        };
        if !self.osc_link_at(at.point) {
            self.links.hovered = self.plain_link_at(at.point);
            return;
        }
        if self.links.pending {
            return;
        }
        if let Some(uri) = self.backend.local_hyperlink(at.point) {
            self.apply_link_hover(at, uri.ok().flatten(), cx);
        } else if let Backend::Attached(connection) = &self.backend {
            let view = connection.access.presentation_lease();
            self.links.pending = self.submit(
                Command::Presentation {
                    view,
                    action: shellfox_runtime::PresentationAction::Hyperlink { point: at.point },
                },
                Pending::LinkHover(at),
            );
        }
    }

    pub(super) fn apply_link_hover(
        &mut self,
        at: HoverPoint,
        uri: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.links.pending = false;
        if self.links.point != Some(at) {
            self.probe_link_hover(cx);
            return;
        }
        if at != self.link_point(at.point) {
            return;
        }
        self.links.hovered = safe_target(uri, self.links.files_enabled).map(|target| LinkRange {
            target,
            start: at.point,
            end: at.point,
        });
        cx.notify();
    }

    pub(crate) fn link_menu_open(&self) -> bool {
        self.links.menu.is_some()
    }

    pub(super) fn link_hover_for_draw(&self) -> Option<&LinkRange> {
        let point = self.links.point?;
        if point == self.link_point(point.point) {
            self.links.hovered.as_ref()
        } else {
            None
        }
    }

    pub(crate) fn hovered_link_cells(&self, window: &gpui::Window) -> Option<(ViewportPoint, ViewportPoint)> {
        if !self.focus.is_focused(window) && !self.link_menu_open() {
            return None;
        }
        self.link_hover_for_draw()
            .map(|link| (link.start, link.end))
    }

    pub(crate) fn enable_file_links(&mut self) {
        self.links.files_enabled = true;
    }

    pub(super) fn open_link(
        &mut self,
        uri: Option<String>,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(uri) = safe_target(uri, self.links.files_enabled) else {
            return;
        };
        if copy_link(modifiers) {
            cx.write_to_clipboard(ClipboardItem::new_string(uri));
        } else if !(cfg!(target_os = "macos") && modifiers.control) {
            if let Some(file) = terminal_file_link::parse(&uri) {
                self.update_link_hover(None, cx);
                cx.emit(file);
            } else if safe_web_url(&uri).is_some() {
                cx.open_url(&uri);
            }
        }
    }

    pub(super) fn activate_link(
        &mut self,
        point: ViewportPoint,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        if !self.osc_link_at(point) {
            self.open_link(
                self.plain_link_at(point).map(|link| link.target),
                modifiers,
                cx,
            );
        } else if let Some(uri) = self.backend.local_hyperlink(point) {
            match uri {
                Ok(uri) => self.open_link(uri, modifiers, cx),
                Err(error) => self.fail(error),
            }
        } else if let Backend::Attached(connection) = &self.backend {
            let view = connection.access.presentation_lease();
            self.submit(
                Command::Presentation {
                    view,
                    action: shellfox_runtime::PresentationAction::Hyperlink { point },
                },
                Pending::Link { modifiers },
            );
        }
    }
}

impl Backend {
    fn local_hyperlink(&mut self, point: ViewportPoint) -> Option<shellfox_ghostty_vt::Result<Option<String>>> {
        match self {
            Self::Fixture(terminal) => Some(terminal.hyperlink_at(point)),
            Self::Peer(peer) => Some(peer.terminal.view_hyperlink(&mut peer.presentation, point)),
            Self::Attached(_) => None,
        }
    }
}

pub(super) fn copy_link(mods: gpui::Modifiers) -> bool {
    mods.shift
        && if cfg!(target_os = "macos") {
            mods.platform
        } else {
            mods.control
        }
}

fn safe_target(value: Option<String>, files: bool) -> Option<String> {
    value.filter(|uri| {
        safe_web_url(uri).is_some() || files && terminal_file_link::parse(uri).is_some()
    })
}

/// Terminal output cannot pass an arbitrary scheme to the system opener.
pub(super) fn safe_web_url(value: &str) -> Option<Url> {
    if value.len() > MAX_LINK_BYTES
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return None;
    }
    let url = Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_opener_accepts_only_bounded_web_urls_without_credentials() {
        assert!(safe_web_url("https://example.com/a").is_some());
        for value in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "shellfox://run",
            "https://",
            "https://user:pass@example.com",
            "https://a\\b",
            "https://a\nb",
            "https://a b",
        ] {
            assert!(safe_web_url(value).is_none(), "{value}");
        }
        assert!(safe_web_url(&format!(
            "https://example.com/{}",
            "x".repeat(MAX_LINK_BYTES)
        ))
        .is_none());
    }

    #[test]
    fn copy_preserves_the_literal_target() {
        assert_eq!(
            safe_target(Some("https://example.com".into()), false).as_deref(),
            Some("https://example.com")
        );
    }
}
