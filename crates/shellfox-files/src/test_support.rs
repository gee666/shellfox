//! Unit-test fixtures: unique directories under the repository's tmp/ folder.
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

pub(crate) struct TempDir {
    pub path: PathBuf,
}
impl TempDir {
    /// A fresh owner-only directory, so it passes the private-parent rules.
    pub fn new() -> Self {
        // No `..` in the spelling: private roots refuse it, and Unix absolute() keeps it.
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("tmp");
        fs::create_dir_all(&base).unwrap();
        let path = base.join(format!("shellfox-files-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&path).unwrap();
        let dir = Self { path };
        crate::private::make_test_private(&dir.handle());
        dir
    }
    /// A no-follow directory handle, pinned against rename on Windows.
    pub fn handle(&self) -> File {
        open_dir(&self.path)
    }
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn open_dir(path: &Path) -> File {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // READ_CONTROL | WRITE_DAC | read; backup semantics + open reparse point.
        options
            .access_mode(0x0012_0089 | 0x40000)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .share_mode(3);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    options.open(path).unwrap()
}

/// A directory symlink on Unix, a mount-point junction on Windows.
pub(crate) fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    junction(target, link);
}

#[cfg(windows)]
pub(crate) fn junction(target: &Path, link: &Path) {
    use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            file: *mut std::ffi::c_void,
            code: u32,
            input: *const std::ffi::c_void,
            input_size: u32,
            output: *mut std::ffi::c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut std::ffi::c_void,
        ) -> i32;
    }
    fs::create_dir(link).unwrap();
    let file = fs::OpenOptions::new()
        .access_mode(0x4000_0000)
        .custom_flags(0x0020_0000 | 0x0200_0000)
        .open(link)
        .unwrap();
    let substitute: Vec<u16> = std::ffi::OsStr::new(&format!("\\??\\{}", target.display()))
        .encode_wide()
        .collect();
    let print: Vec<u16> = target.as_os_str().encode_wide().collect();
    let data_len = 8 + (substitute.len() + print.len() + 2) * 2;
    let mut data = Vec::with_capacity(data_len + 8);
    data.extend_from_slice(&0xa000_0003u32.to_le_bytes());
    data.extend_from_slice(&(data_len as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    data.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
    data.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    for value in substitute.into_iter().chain([0]).chain(print).chain([0]) {
        data.extend_from_slice(&value.to_le_bytes());
    }
    let mut returned = 0;
    // SAFETY: a live directory handle and a bounded mount-point reparse buffer.
    let result = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x900a4,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(result, 0, "{}", std::io::Error::last_os_error());
}

/// Replaces the DACL with owner full control plus Everyone read (and write if asked).
#[cfg(windows)]
pub(crate) fn grant_everyone(file: &File, write: bool) {
    use std::{os::windows::io::AsRawHandle, ptr::null_mut};
    use windows_sys::Win32::Security::{
        AddAccessAllowedAceEx,
        Authorization::{SetSecurityInfo, SE_FILE_OBJECT},
        CreateWellKnownSid, InitializeAcl, WinWorldSid, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION,
    };
    let owner = crate::private::current_sid().unwrap();
    let mut world = [0usize; 9];
    let mut length = std::mem::size_of_val(&world) as u32;
    let mut storage = [0usize; 64];
    let acl = storage.as_mut_ptr().cast::<ACL>();
    // SAFETY: aligned local buffers own both SIDs and the ACL for the whole update.
    unsafe {
        assert_ne!(
            CreateWellKnownSid(
                WinWorldSid,
                null_mut(),
                world.as_mut_ptr().cast(),
                &mut length
            ),
            0
        );
        assert_ne!(
            InitializeAcl(acl, std::mem::size_of_val(&storage) as u32, 2),
            0
        );
        assert_ne!(
            AddAccessAllowedAceEx(acl, 2, 3, 0x1f01ff, owner.as_ptr().cast_mut().cast()),
            0
        );
        let mask = 0x120089 | if write { 2 } else { 0 };
        assert_ne!(
            AddAccessAllowedAceEx(acl, 2, 3, mask, world.as_mut_ptr().cast()),
            0
        );
        assert_eq!(
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl,
                null_mut()
            ),
            0
        );
    }
}
