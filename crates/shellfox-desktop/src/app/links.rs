use super::{Backend, TerminalView, runtime::Pending};
use gpui::{ClipboardItem, Context};
use shellfox_ghostty_vt::{Screen, ViewportPoint};
use shellfox_runtime::Command;

impl TerminalView {
    pub(super) fn open_link(
        uri: Option<String>,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(uri) = uri.and_then(|uri| safe_web_url(&uri)) else {
            return;
        };
        if modifiers.shift
            && if cfg!(target_os = "macos") {
                modifiers.platform
            } else {
                modifiers.control
            }
        {
            cx.write_to_clipboard(ClipboardItem::new_string(uri));
        } else if !(cfg!(target_os = "macos") && modifiers.control) {
            cx.open_url(&uri);
        }
    }

    pub(super) fn activate_link(
        &mut self,
        point: ViewportPoint,
        modifiers: gpui::Modifiers,
        cx: &mut Context<Self>,
    ) {
        let fallback = at_cell(
            &self.screen,
            usize::from(point.y) * usize::from(self.screen.cols) + usize::from(point.x),
        );
        match &mut self.backend {
            Backend::Fixture(terminal) => match terminal.hyperlink_at(point) {
                Ok(uri) => Self::open_link(resolve_target(uri, fallback), modifiers, cx),
                Err(error) => self.fail(error),
            },
            Backend::Attached(connection) => {
                let view = connection.access.presentation_lease();
                self.submit(
                    Command::Presentation {
                        view,
                        action: shellfox_runtime::PresentationAction::Hyperlink { point },
                    },
                    Pending::Link {
                        modifiers,
                        fallback,
                    },
                );
            }
        }
    }
}

use url::Url;

const MAX_LINK_BYTES: usize = 2048;

pub(super) fn resolve_target(native: Option<String>, fallback: Option<String>) -> Option<String> {
    // An unsafe OSC 8 URI must not turn its visible label into a different link.
    native.or(fallback).and_then(|uri| safe_web_url(&uri))
}

/// Shell output is untrusted. Never pass a non-web scheme to the OS opener.
pub fn safe_web_url(value: &str) -> Option<String> {
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
    Some(url.into())
}

pub fn at_cell(screen: &Screen, index: usize) -> Option<String> {
    let cols = usize::from(screen.cols);
    if cols == 0 || index >= screen.cells.len() {
        return None;
    }
    let row_start = index / cols * cols;
    let mut text = String::new();
    let mut hit = None;
    for (offset, cell) in screen.cells[row_start..row_start + cols].iter().enumerate() {
        let start = text.len();
        if cell.width != 0 {
            text.push_str(if cell.text.is_empty() {
                " "
            } else {
                &cell.text
            });
        }
        if row_start + offset == index {
            hit = Some(start);
        }
        if cell.width == 2 && row_start + offset + 1 == index {
            hit = Some(start);
        }
        // Keep the leading glyph position for a wide spacer.
        if cell.width == 0 && row_start + offset == index && offset > 0 {
            hit = Some(text.char_indices().next_back()?.0);
        }
    }
    let hit = hit?;
    for (start, _) in text.match_indices("http") {
        let tail = &text[start..];
        if !(tail.starts_with("http://") || tail.starts_with("https://")) {
            continue;
        }
        let length = tail
            .find(|c: char| c.is_whitespace() || c.is_control() || "<>\"'`".contains(c))
            .unwrap_or(tail.len());
        let mut candidate = &tail[..length];
        loop {
            let last = candidate.chars().next_back()?;
            let unmatched =
                |open, close| candidate.matches(open).count() < candidate.matches(close).count();
            if ".,;:!?*".contains(last)
                || last == ')' && unmatched('(', ')')
                || last == ']' && unmatched('[', ']')
            {
                candidate = &candidate[..candidate.len() - last.len_utf8()];
            } else {
                break;
            }
        }
        if (start..start + candidate.len()).contains(&hit)
            && let Some(url) = safe_web_url(candidate)
        {
            return Some(url);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_ghostty_vt::Terminal;

    #[test]
    fn external_opener_accepts_only_bounded_web_urls_without_credentials() {
        assert_eq!(
            safe_web_url("https://example.com/a").as_deref(),
            Some("https://example.com/a")
        );
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
        assert!(
            safe_web_url(&format!(
                "https://example.com/{}",
                "x".repeat(MAX_LINK_BYTES)
            ))
            .is_none()
        );
    }

    #[test]
    fn osc8_target_is_validated_and_never_replaced_with_its_label() {
        assert!(
            resolve_target(
                Some("javascript:alert(1)".into()),
                Some("https://label.example/".into())
            )
            .is_none()
        );
        assert_eq!(
            resolve_target(
                Some("https://target.example/".into()),
                Some("https://label.example/".into())
            )
            .as_deref(),
            Some("https://target.example/")
        );
        assert_eq!(
            resolve_target(None, Some("https://label.example/".into())).as_deref(),
            Some("https://label.example/")
        );
    }

    #[test]
    fn detects_link_at_cell_without_trailing_prose_punctuation() {
        let mut terminal = Terminal::new(80, 2).unwrap();
        terminal
            .feed("界 (https://example.com/a_(b)). next".as_bytes())
            .unwrap();
        let screen = terminal.render().unwrap();
        assert_eq!(
            at_cell(&screen, 10).as_deref(),
            Some("https://example.com/a_(b)")
        );
        assert!(at_cell(&screen, 1).is_none());
        assert!(at_cell(&screen, 40).is_none());
    }
}
