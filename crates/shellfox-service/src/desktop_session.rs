//! Whether the graphical login session the desktop runs in has ended: logind
//! is closing it, or its Wayland or X11 display no longer takes clients. The
//! desktop UI and its host both ask before treating a lost UI as a crash, so a
//! logout stops the host and its shells, as 0.9.x did, instead of relaunching.
#[cfg(any(target_os = "linux", test))]
use std::path::{Path, PathBuf};

/// Checks for up to two seconds, since a display goes away just after its
/// clients do. Always false outside Linux.
pub fn ended() -> bool {
    #[cfg(target_os = "linux")]
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        if linux::ended_now() {
            return true;
        }
    }
    false
}

#[cfg(any(target_os = "linux", test))]
/// The `STATE=` value of a logind session record.
fn logind_state(record: &str) -> Option<&str> {
    record.lines().find_map(|line| line.strip_prefix("STATE="))
}

#[cfg(any(target_os = "linux", test))]
/// The display number of a local X11 display such as `:0`, `:1.0` or `unix:2`.
/// Remote displays (`host:10.0`) have no local socket to probe.
fn x11_display(display: &str) -> Option<u32> {
    let number = display
        .strip_prefix(':')
        .or_else(|| display.strip_prefix("unix:"))?;
    number.split('.').next()?.parse().ok()
}

#[cfg(any(target_os = "linux", test))]
/// The socket path a Wayland client connects to.
fn wayland_socket(display: &Path, runtime_dir: Option<&Path>) -> Option<PathBuf> {
    if display.is_absolute() {
        Some(display.to_owned())
    } else {
        runtime_dir.map(|dir| dir.join(display))
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{
        io::ErrorKind,
        os::unix::net::{SocketAddr, UnixStream},
        path::Path,
    };

    pub(super) fn ended_now() -> bool {
        logind_closing() || display_gone()
    }

    fn logind_closing() -> bool {
        let Some(id) = std::env::var("XDG_SESSION_ID")
            .ok()
            .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        else {
            return false;
        };
        let sessions = Path::new("/run/systemd/sessions");
        if !sessions.is_dir() {
            return false;
        }
        match std::fs::read_to_string(sessions.join(id)) {
            Ok(record) => super::logind_state(&record) == Some("closing"),
            Err(error) => error.kind() == ErrorKind::NotFound,
        }
    }

    /// Only a missing or refusing socket counts; an unknown display does not.
    fn display_gone() -> bool {
        if let Some(display) = std::env::var_os("WAYLAND_DISPLAY") {
            let runtime = std::env::var_os("XDG_RUNTIME_DIR");
            return super::wayland_socket(Path::new(&display), runtime.as_deref().map(Path::new))
                .is_some_and(|socket| refused(UnixStream::connect(socket)));
        }
        let Some(number) = std::env::var("DISPLAY")
            .ok()
            .as_deref()
            .and_then(super::x11_display)
        else {
            return false;
        };
        let socket = format!("/tmp/.X11-unix/X{number}");
        refused(UnixStream::connect(&socket)) && refused(connect_abstract(&socket))
    }

    fn connect_abstract(name: &str) -> std::io::Result<UnixStream> {
        use std::os::linux::net::SocketAddrExt;
        UnixStream::connect_addr(&SocketAddr::from_abstract_name(name)?)
    }

    pub(super) fn refused(connection: std::io::Result<UnixStream>) -> bool {
        connection.is_err_and(|error| {
            matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_logind_state() {
        assert_eq!(
            logind_state("UID=1000\nSTATE=closing\nACTIVE=0\n"),
            Some("closing")
        );
        assert_eq!(logind_state("STATE=active\n"), Some("active"));
        assert_eq!(logind_state("UID=1000\n"), None);
    }

    #[test]
    fn only_local_x11_displays_have_a_socket() {
        assert_eq!(x11_display(":0"), Some(0));
        assert_eq!(x11_display(":1.0"), Some(1));
        assert_eq!(x11_display("unix:2"), Some(2));
        assert_eq!(x11_display("localhost:10.0"), None);
        assert_eq!(x11_display(""), None);
    }

    #[test]
    fn wayland_names_resolve_in_the_runtime_dir() {
        let runtime = Path::new("/run/user/1000");
        assert_eq!(
            wayland_socket(Path::new("wayland-0"), Some(runtime)),
            Some(runtime.join("wayland-0"))
        );
        assert_eq!(wayland_socket(Path::new("wayland-0"), None), None);
        let absolute = if cfg!(windows) {
            r"C:\w\wayland-1"
        } else {
            "/tmp/wayland-1"
        };
        assert_eq!(
            wayland_socket(Path::new(absolute), None),
            Some(PathBuf::from(absolute))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_live_socket_is_not_gone_and_a_closed_one_is() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("display");
        assert!(linux::refused(std::os::unix::net::UnixStream::connect(
            &path
        )));
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(!linux::refused(std::os::unix::net::UnixStream::connect(
            &path
        )));
        drop(listener);
        assert!(linux::refused(std::os::unix::net::UnixStream::connect(
            &path
        )));
    }
}
