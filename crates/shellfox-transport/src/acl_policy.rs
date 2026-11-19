//! Darwin ACLs are queried and changed on the original open descriptor.
use std::ffi::{c_int, c_void};
use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::ptr::{null_mut, NonNull};

const ACL_FIRST_ENTRY: c_int = 0;
const ACL_NEXT_ENTRY: c_int = -1;
const ACL_EXTENDED_DENY: c_int = 2;

extern "C" {
    fn acl_get_fd(fd: c_int) -> *mut c_void;
    fn acl_init(count: c_int) -> *mut c_void;
    fn acl_set_fd(fd: c_int, acl: *mut c_void) -> c_int;
    fn acl_get_entry(acl: *mut c_void, which: c_int, entry: *mut *mut c_void) -> c_int;
    fn acl_get_tag_type(entry: *mut c_void, tag: *mut c_int) -> c_int;
    fn acl_free(acl: *mut c_void) -> c_int;
}

struct Acl(NonNull<c_void>);
impl Drop for Acl {
    fn drop(&mut self) {
        unsafe { acl_free(self.0.as_ptr()) };
    }
}

pub(super) fn verify(file: &impl AsFd, allow_deny_only: bool) -> io::Result<()> {
    let acl = unsafe { acl_get_fd(file.as_fd().as_raw_fd()) };
    let Some(acl) = NonNull::new(acl).map(Acl) else {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(error)
        };
    };
    let mut which = ACL_FIRST_ENTRY;
    loop {
        let mut entry = null_mut();
        if unsafe { acl_get_entry(acl.0.as_ptr(), which, &mut entry) } != 0 {
            let error = io::Error::last_os_error();
            // Darwin returns EINVAL at end of a validated ACL, not 0 as on Linux.
            return if error.kind() == io::ErrorKind::InvalidInput {
                Ok(())
            } else {
                Err(error)
            };
        }
        let mut tag = 0;
        if !allow_deny_only
            || unsafe { acl_get_tag_type(entry, &mut tag) } != 0
            || tag != ACL_EXTENDED_DENY
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        which = ACL_NEXT_ENTRY;
    }
}

pub(super) fn clear_owned(file: &impl AsFd) -> io::Result<()> {
    use rustix::fs::{fstat, FileType};
    let info = fstat(file)?;
    let kind = FileType::from_raw_mode(info.st_mode);
    if info.st_uid != rustix::process::geteuid().as_raw()
        || !(kind == FileType::Directory || kind == FileType::RegularFile && info.st_nlink == 1)
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    match verify(file, false) {
        Ok(()) => return Ok(()),
        Err(error) if error.kind() != io::ErrorKind::PermissionDenied => return Err(error),
        Err(_) => (),
    }
    // acl_delete_fd_np is an ENOTSUP stub on Darwin. An empty ACL removes all
    // grants and inherited entries without changing owner/group or POSIX mode.
    let acl = NonNull::new(unsafe { acl_init(0) })
        .map(Acl)
        .ok_or_else(io::Error::last_os_error)?;
    if unsafe { acl_set_fd(file.as_fd().as_raw_fd(), acl.0.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    verify(file, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    };

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            if let Ok(file) = fs::File::open(&self.0) {
                let _ = clear_owned(&file);
            }
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn macos_checks_native_acl_and_clears_only_the_held_resource() -> io::Result<()> {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        fs::create_dir_all(&base)?;
        let dir = Scratch(base.join(format!("mac-acl-{}", std::process::id())));
        fs::create_dir(&dir.0)?;
        let held = fs::File::open(&dir.0)?;
        verify(&held, false)?;
        assert!(Command::new("/bin/chmod")
            .arg("+a")
            .arg("everyone deny delete")
            .arg(&dir.0)
            .status()?
            .success());
        assert!(verify(&held, false).is_err());
        verify(&held, true)?;
        assert!(Command::new("/bin/chmod")
            .arg("+a")
            .arg("everyone allow list")
            .arg(&dir.0)
            .status()?
            .success());
        assert!(verify(&held, true).is_err());
        clear_owned(&held)?;
        verify(&held, false)
    }
}
