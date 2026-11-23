//! OS-private local streams only. There is deliberately no TCP implementation.
use anyhow::{bail, Result};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};

#[cfg(target_os = "macos")]
mod darwin;

pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send {
    fn verify_peer(&self) -> Result<()>;
}
#[cfg(windows)]
impl Stream for tokio::net::windows::named_pipe::NamedPipeServer {
    fn verify_peer(&self) -> Result<()> {
        crate::pipe_identity::verify_client(self)
    }
}
#[cfg(windows)]
impl Stream for tokio::net::windows::named_pipe::NamedPipeClient {
    fn verify_peer(&self) -> Result<()> {
        crate::pipe_identity::verify_server(self)
    }
}
#[cfg(unix)]
impl Stream for tokio::net::UnixStream {
    fn verify_peer(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        let same_user = darwin::peer_uid(self)? == rustix::process::geteuid().as_raw();
        #[cfg(not(target_os = "macos"))]
        let same_user = {
            // A fresh socketpair reports our effective UID without global IDs.
            let (own, _) = tokio::net::UnixStream::pair()?;
            self.peer_cred()?.uid() == own.peer_cred()?.uid()
        };
        if !same_user {
            bail!("Local socket peer has a different user.");
        }
        Ok(())
    }
}
pub(crate) type LocalStream = Box<dyn Stream>;

#[cfg(windows)]
pub(crate) struct Listener {
    next: tokio::net::windows::named_pipe::NamedPipeServer,
    address: String,
    security: Security,
}
#[cfg(windows)]
struct Security(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);
#[cfg(windows)]
impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::LocalFree(self.0);
        }
    }
}
#[cfg(windows)]
impl Listener {
    /// Caller has just obtained helper proof that directory owner and protected
    /// DACL grant access to the current user only. Copy that exact DACL to pipe.
    pub fn bind(directory: &Path) -> Result<Self> {
        use std::{os::windows::ffi::OsStrExt, ptr};
        use windows_sys::Win32::Security::{
            Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
            DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        };
        let wide: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = ptr::null_mut();
        let status = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32).into());
        }
        let security = Security(descriptor);
        let address = format!(r"\\.\pipe\shellfox-rust-v1-{}", uuid::Uuid::new_v4());
        let next = create_pipe(&address, &security, true)?;
        Ok(Self {
            next,
            address,
            security,
        })
    }
    pub fn address(&self) -> String {
        self.address.clone()
    }
    pub async fn accept(&mut self) -> Result<LocalStream> {
        self.next.connect().await?;
        let next = create_pipe(&self.address, &self.security, false)?;
        Ok(Box::new(std::mem::replace(&mut self.next, next)))
    }
}
#[cfg(windows)]
fn create_pipe(
    address: &str,
    security: &Security,
    first: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let mut options = tokio::net::windows::named_pipe::ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true)
        // The listener separately bounds request and lifetime tasks. A low
        // kernel instance cap prevented it from rotating the accept pipe.
        .max_instances(160);
    // The descriptor remains alive throughout creation. Tokio passes it directly
    // to CreateNamedPipeW; Windows copies it into the new pipe's kernel object.
    Ok(unsafe {
        options.create_with_security_attributes_raw(
            address,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }?)
}
#[cfg(windows)]
pub(crate) async fn connect(address: &str, _directory: &Path) -> Result<LocalStream> {
    let suffix = address.strip_prefix(r"\\.\pipe\shellfox-rust-v1-");
    if !suffix.is_some_and(|value| uuid::Uuid::parse_str(value).is_ok()) {
        bail!("Invalid named-pipe endpoint.");
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(250);
    let pipe = loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(address) {
            Ok(pipe) => break pipe,
            Err(error)
                if error.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32)
                    && tokio::time::Instant::now() < deadline =>
            {
                // The listener marks the next instance available after accepting
                // the previous connection. No request has been sent yet.
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            Err(error) => return Err(error.into()),
        }
    };
    let stream: LocalStream = Box::new(pipe);
    stream.verify_peer()?;
    Ok(stream)
}

#[cfg(unix)]
pub(crate) struct Listener {
    socket: tokio::net::UnixListener,
    path: PathBuf,
    _directory: std::fs::File,
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn anchored_socket_path(
    directory: &Path,
    original: &std::fs::File,
    leaf: &std::ffi::OsStr,
) -> Result<(std::fs::File, PathBuf)> {
    use rustix::fs::{openat, Mode, OFlags};
    use std::os::fd::AsRawFd;
    verify_socket_parent(directory, original)?;
    let fd = openat(
        original,
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let file = std::fs::File::from(fd);
    verify_socket_parent(directory, &file)?;
    use std::os::unix::fs::MetadataExt;
    let expected = original.metadata()?;
    let actual = file.metadata()?;
    if expected.dev() != actual.dev() || expected.ino() != actual.ino() {
        bail!("Socket directory differs from original held parent.");
    }
    // Kernel pathname stays short while the real socket remains inside the
    // original verified private namespace with the SAME OS permissions.
    let kernel = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())).join(leaf);
    Ok((file, kernel))
}
#[cfg(unix)]
fn verify_socket_parent(directory: &Path, original: &std::fs::File) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let before = std::fs::symlink_metadata(directory)?;
    let held = original.metadata()?;
    if !before.is_dir()
        || before.file_type().is_symlink()
        || !held.is_dir()
        || before.dev() != held.dev()
        || before.ino() != held.ino()
        || before.uid() != held.uid()
        || before.mode() != held.mode()
        || held.mode() & 0o7777 != 0o700
        || held.uid() != rustix::process::geteuid().as_raw()
    {
        bail!("Socket parent differs from original verified directory.");
    }
    Ok(())
}
#[cfg(unix)]
impl Listener {
    pub fn bind_verified(directory: &Path, original: std::fs::File) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        let leaf = format!("{}.sock", uuid::Uuid::new_v4());
        let path = directory.join(&leaf);
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let (held_directory, kernel_path) =
            anchored_socket_path(directory, &original, std::ffi::OsStr::new(&leaf))?;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        let (held_directory, kernel_path) = {
            verify_socket_parent(directory, &original)?;
            (original, path.clone())
        };
        #[cfg(target_os = "macos")]
        let socket = darwin::bind(&held_directory, std::ffi::OsStr::new(&leaf))?;
        #[cfg(not(target_os = "macos"))]
        let socket = tokio::net::UnixListener::bind(&kernel_path)?;
        std::fs::set_permissions(&kernel_path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self {
            socket,
            path,
            _directory: held_directory,
        })
    }
    pub fn address(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
    pub async fn accept(&mut self) -> Result<LocalStream> {
        Ok(Box::new(self.socket.accept().await?.0))
    }
}
#[cfg(unix)]
pub(crate) async fn connect_verified(
    address: &str,
    directory: &Path,
    original: &std::fs::File,
) -> Result<LocalStream> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let path = PathBuf::from(address);
    if path.parent() != Some(directory) {
        bail!("Endpoint escaped private directory.");
    }
    let info = std::fs::symlink_metadata(&path)?;
    if !info.file_type().is_socket()
        || info.mode() & 0o7777 != 0o600
        || info.uid() != rustix::process::geteuid().as_raw()
    {
        bail!("Unsafe local socket permissions.");
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let (held_directory, kernel_path) = anchored_socket_path(
        directory,
        original,
        path.file_name()
            .ok_or_else(|| anyhow::anyhow!("Socket leaf missing"))?,
    )?;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let kernel_path = {
        verify_socket_parent(directory, original)?;
        path
    };
    let pinned = std::fs::symlink_metadata(&kernel_path)?;
    if !pinned.file_type().is_socket()
        || pinned.mode() & 0o7777 != 0o600
        || pinned.dev() != info.dev()
        || pinned.ino() != info.ino()
        || pinned.uid() != info.uid()
    {
        bail!("Local socket changed while anchoring its private parent.");
    }
    #[cfg(target_os = "macos")]
    let stream: LocalStream = Box::new(
        darwin::connect(
            original,
            kernel_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Socket leaf missing"))?,
        )
        .await?,
    );
    #[cfg(not(target_os = "macos"))]
    let stream: LocalStream = Box::new(tokio::net::UnixStream::connect(kernel_path).await?);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    drop(held_directory);
    stream.verify_peer()?;
    Ok(stream)
}

#[cfg(windows)]
impl Listener {
    pub fn bind_verified(directory: &Path) -> Result<Self> {
        Self::bind(directory)
    }
}
#[cfg(unix)]
impl Listener {
    pub fn bind(directory: &Path) -> Result<Self> {
        use rustix::fs::{open, Mode, OFlags};
        let original = std::fs::File::from(open(
            directory,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        Self::bind_verified(directory, original)
    }
}
#[cfg(windows)]
pub(crate) async fn connect_verified(address: &str, directory: &Path) -> Result<LocalStream> {
    connect(address, directory).await
}
#[cfg(unix)]
pub(crate) async fn connect(address: &str, directory: &Path) -> Result<LocalStream> {
    use rustix::fs::{open, Mode, OFlags};
    let original = std::fs::File::from(open(
        directory,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    connect_verified(address, directory, &original).await
}
#[cfg(not(any(unix, windows)))]
compile_error!("No verified private local endpoint backend for this platform.");
