use super::terminal::Terminal;
use anyhow::Result;
use shellfox_ssh::ProfileDto;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(super) struct Picker<'a> {
    pub(super) terminal: &'a Terminal,
    pub(super) lines: usize,
}
impl Picker<'_> {
    fn erase(&mut self) -> Result<()> {
        if self.lines > 0 {
            self.terminal
                .write(&format!("\x1b[{}A\r\x1b[J", self.lines))?;
            self.lines = 0;
        }
        Ok(())
    }
    pub(super) fn draw(
        &mut self,
        profiles: &[ProfileDto],
        selected: usize,
        (width, rows): (u16, u16),
    ) -> Result<()> {
        self.erase()?;
        let capacity = usize::from(rows.saturating_sub(6)).max(1);
        let start = selected
            .saturating_sub(capacity / 2)
            .min(profiles.len().saturating_sub(capacity));
        let width = usize::from(width).max(1);
        let window = &profiles[start..profiles.len().min(start + capacity)];
        let mut lines = vec![String::new(), "  ssh".into()];
        let labels: Vec<_> = window
            .iter()
            .map(|p| {
                (
                    escape(&p.name),
                    target(&p.user, &p.host, p.port),
                    escape(p.remote_cwd.as_deref().unwrap_or("")),
                )
            })
            .collect();
        let names = labels
            .iter()
            .map(|l| len(&l.0))
            .max()
            .unwrap_or(0)
            .min((width / 3).max(4));
        let targets = labels
            .iter()
            .map(|l| len(&l.1))
            .max()
            .unwrap_or(0)
            .min(width.saturating_sub(names + 10).max(5));
        let cwd_cols = width.saturating_sub(names + targets + 10);
        for (i, (name, endpoint, cwd)) in labels.into_iter().enumerate() {
            let name = pad(&fit(&name, names), names);
            let endpoint = pad(&fit(&endpoint, targets), targets);
            let cwd = fit(&cwd, cwd_cols);
            let mut line = if start + i == selected {
                format!(
                    "  {}",
                    self.terminal.accent(&format!("› {name}  {endpoint}"))
                )
            } else {
                format!("    {name}  {}", self.terminal.dim(&endpoint))
            };
            if !cwd.is_empty() {
                line.push_str(&format!("  {}", self.terminal.dim(&cwd)));
            }
            lines.push(line);
        }
        lines.push(String::new());
        lines.push(format!(
            "  {}",
            self.terminal.dim("↑↓ select  enter connect  esc cancel")
        ));
        let text = wrap(&lines.join("\n"), width);
        self.lines = text.split('\n').count();
        self.terminal.write(&format!("{text}\n"))
    }
}
impl Drop for Picker<'_> {
    fn drop(&mut self) {
        let _ = self.erase();
    }
}

fn len(text: &str) -> usize {
    text.encode_utf16().count()
}
fn pad(text: &str, n: usize) -> String {
    format!("{text}{}", " ".repeat(n.saturating_sub(len(text))))
}
fn fit(text: &str, n: usize) -> String {
    if len(text) <= n {
        return text.to_owned();
    }
    let head: Vec<_> = text.encode_utf16().take(n.saturating_sub(1)).collect();
    format!("{}…", String::from_utf16_lossy(&head))
}
pub(super) fn wrap(text: &str, width: usize) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i != 0 {
            out.push('\n');
        }
        let mut tail = line;
        let mut used = 0;
        while !tail.is_empty() {
            let part = if tail.starts_with("\x1b[") {
                let end = tail.find('m').map_or(tail.len(), |i| i + 1);
                out.push_str(&tail[..end]);
                tail = &tail[end..];
                continue;
            } else {
                tail.find("\x1b[").map_or(tail, |i| &tail[..i])
            };
            for grapheme in part.graphemes(true) {
                let len = grapheme.width();
                if used > 0 && used + len > width {
                    out.push('\n');
                    used = 0;
                }
                used += len;
                out.push_str(grapheme);
            }
            tail = &tail[part.len()..];
        }
    }
    out
}
pub(super) fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_control() || matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            use std::fmt::Write as _;
            let _ = write!(escaped, "\\u{:04x}", u32::from(c));
        } else {
            escaped.push(c);
        }
    }
    escaped
}
pub(super) fn target(user: &str, host: &str, port: u16) -> String {
    let user = if user.is_empty() {
        String::new()
    } else {
        format!("{user}@")
    };
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let port = if port == 22 {
        String::new()
    } else {
        format!(":{port}")
    };
    escape(&format!("{user}{host}{port}"))
}
