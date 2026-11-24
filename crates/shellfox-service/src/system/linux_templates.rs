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
/// Files and Caja run scripts in the viewed local folder, with the selection
/// as arguments relative to it. Using those instead of the URI variables means
/// no percent-decoding, so names with spaces or non-UTF-8 bytes pass unchanged.
pub(super) fn script(launcher: &Path, caja: bool) -> SystemResult<String> {
    let launcher = shell_quote(path_text(launcher)?);
    let prefix = if caja { "CAJA" } else { "NAUTILUS" };
    Ok(format!(
        r#"#!/bin/sh
# Shellfox/linux-v1
# Remote folders have no local working directory.
case "${{{prefix}_SCRIPT_CURRENT_URI:-}}" in
  [Ff][Ii][Ll][Ee]:///*|[Ff][Ii][Ll][Ee]://localhost/*) ;;
  *) exit 1 ;;
esac
[ "$#" -le 1 ] || exit 1
folder=$PWD
if [ "$#" = 1 ]; then
  case "$1" in /*) folder=$1 ;; *) folder=$PWD/$1 ;; esac
fi
case "$folder" in *[[:cntrl:]]*) exit 1 ;; esac
[ -d "$folder" ] || exit 1
exec {launcher} start "$folder"
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
