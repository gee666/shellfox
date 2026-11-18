use super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
use std::path::Path;
pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn path_text(path: &Path) -> SystemResult<&str> {
    super::validation::absolute_path(path)?;
    path.to_str()
        .ok_or_else(|| error(ErrorCode::Validation, "CLI path encoding unavailable."))
}
pub(super) fn nautilus(launcher: &Path) -> SystemResult<String> {
    let launcher = serde_json::to_string(path_text(launcher)?)
        .map_err(|_| error(ErrorCode::Validation, "Invalid file-menu path."))?;
    Ok(format!(
        r#"# Shellfox/linux-v1
import gi
try:
    gi.require_version('Nautilus', '4.0')
except ValueError:
    gi.require_version('Nautilus', '3.0')
from gi.repository import Nautilus, GObject
from urllib.parse import urlparse, unquote
import subprocess
import os
LAUNCHER = {launcher}
def local_path(item):
    if item is None:
        return None
    raw = item.get_uri()
    uri = urlparse(raw)
    if not raw.lower().startswith('file://') or uri.scheme != 'file' or uri.netloc not in ('', 'localhost') or uri.query or uri.fragment:
        return None
    value = unquote(uri.path)
    return value if value.startswith('/') and not any(ord(c) < 32 or ord(c) == 127 for c in value) and os.path.isdir(value) else None
class ShellfoxMenu(GObject.GObject, Nautilus.MenuProvider):
    def menu(self, item):
        value = local_path(item)
        if not value:
            return []
        action = Nautilus.MenuItem(name='Shellfox::Open', label='Open in Shellfox', tip='Start a terminal session in this folder')
        action.connect('activate', lambda _item: subprocess.Popen([LAUNCHER, 'start', value], start_new_session=True))
        return [action]
    # API 3 passes window,files; API 4 passes files. Both use the last argument.
    def get_file_items(self, *args):
        files = args[-1] if args else []
        return self.menu(files[0]) if files and len(files) == 1 and files[0].is_directory() else []
    def get_background_items(self, *args):
        return self.menu(args[-1]) if args else []
"#
    ))
}
pub(super) fn script(launcher: &Path, caja: bool) -> SystemResult<String> {
    let launcher = serde_json::to_string(path_text(launcher)?)
        .map_err(|_| error(ErrorCode::Validation, "Invalid file-menu path."))?;
    let prefix = if caja { "CAJA" } else { "NAUTILUS" };
    Ok(format!(
        r#"#!/usr/bin/env python3
# Shellfox/linux-v1
import os, subprocess, sys
from urllib.parse import urlparse, unquote
selected = [p for p in os.environ.get('{prefix}_SCRIPT_SELECTED_FILE_PATHS', '').splitlines() if p]
if len(selected) > 1: sys.exit(1)
folder = selected[0] if selected else ''
if not folder:
    raw = os.environ.get('{prefix}_SCRIPT_CURRENT_URI', '')
    uri = urlparse(raw)
    if not raw.lower().startswith('file://') or uri.scheme != 'file' or uri.netloc not in ('', 'localhost') or uri.query or uri.fragment: sys.exit(1)
    folder = unquote(uri.path)
if not os.path.isabs(folder) or any(ord(c) < 32 or ord(c) == 127 for c in folder) or not os.path.isdir(folder): sys.exit(1)
subprocess.Popen([{launcher}, 'start', folder], start_new_session=True)
"#
    ))
}
fn desktop_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' | '\\' | '`' | '$' => quoted.push('\\'),
            '%' => quoted.push('%'),
            _ => {}
        }
        quoted.push(ch);
    }
    quoted.push('"');
    quoted
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
pub(super) fn dolphin(launcher: &Path) -> SystemResult<String> {
    Ok(format!("# Shellfox/linux-v1\n[Desktop Entry]\nType=Service\nMimeType=inode/directory;\nX-KDE-ServiceTypes=KonqPopupMenu/Plugin\nActions=Shellfox;\nX-KDE-Protocols=file\nX-KDE-Priority=TopLevel\n[Desktop Action Shellfox]\nName=Open in Shellfox\nIcon=utilities-terminal\nExec={} start %f\n",desktop_quote(path_text(launcher)?)))
}
pub(super) fn nemo(launcher: &Path, background: bool) -> SystemResult<String> {
    Ok(format!("# Shellfox/linux-v1\n[Nemo Action]\nName=Open in Shellfox\nComment=Start a terminal session in this folder\nExec={} start {}\nSelection={}\nUriScheme=file\nExtensions={};\nIcon-Name=utilities-terminal\n",desktop_quote(path_text(launcher)?),if background{"%P"}else{"%F"},if background{"none"}else{"s"},if background{"any"}else{"dir"}))
}
