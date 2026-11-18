//! Conservative native namespace proof for two not-yet-created components.
use cap_std::fs::Dir;
use std::ffi::OsStr;
fn safe(name: &OsStr) -> Option<&str> {
    name.to_str().filter(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    })
}
/// Some(true) proves different native names. Case-only, non-ASCII, alternate-name
/// looking or unknown filesystem rules remain None, never guessed Disjoint.
pub(crate) fn distinct(parent: &Dir, a: &OsStr, b: &OsStr) -> Option<bool> {
    let (a, b) = (safe(a)?, safe(b)?);
    if !known_namespace(parent) {
        return None;
    }
    if native_case_equal(a, b)? {
        None
    } else {
        Some(true)
    }
}
#[cfg(windows)]
fn filesystem_name(parent: &Dir) -> Option<String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;
    let mut name = [0_u16; 32];
    // SAFETY: the retained directory handle and bounded output buffer are live.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            parent.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return None;
    }
    let end = name
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(name.len());
    String::from_utf16(&name[..end]).ok()
}
#[cfg(windows)]
fn known_namespace(parent: &Dir) -> bool {
    matches!(filesystem_name(parent).as_deref(), Some("NTFS" | "ReFS"))
}
#[cfg(windows)]
fn native_case_equal(a: &str, b: &str) -> Option<bool> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CompareStringOrdinal(
            a: *const u16,
            a_len: i32,
            b: *const u16,
            b_len: i32,
            ignore_case: i32,
        ) -> i32;
    }
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    // Safe ASCII names cannot be Windows generated DOS alternate names: those
    // contain '~' unless the full name is already equivalent under this compare.
    let result =
        unsafe { CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) };
    match result {
        1 | 3 => Some(false),
        2 => Some(true),
        _ => None,
    }
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn known_namespace(parent: &Dir) -> bool {
    use std::os::fd::AsRawFd;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: retained fd and output allocation remain live throughout fstatfs.
    if unsafe { libc::fstatfs(parent.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return false;
    }
    let info = unsafe { info.assume_init() };
    matches!(
        info.f_type as u64,
        0xef53 | 0x58465342 | 0x9123683e | 0x01021994 | 0x794c7630 | 0xf2f52010
    )
}
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn known_namespace(parent: &Dir) -> bool {
    use std::os::fd::AsRawFd;
    let mut info = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    if unsafe { libc::fstatfs(parent.as_raw_fd(), info.as_mut_ptr()) } != 0 {
        return false;
    }
    let info = unsafe { info.assume_init() };
    let name: Vec<u8> = info
        .f_fstypename
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    matches!(name.as_slice(), b"apfs" | b"hfs")
}
#[cfg(all(
    unix,
    any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )
))]
fn native_case_equal(a: &str, b: &str) -> Option<bool> {
    // Native C-locale comparison: int strcasecmp_l(const char *, const char *, locale_t).
    // libc's Rust symbol exposure varies across these supported Unix targets.
    unsafe extern "C" {
        #[link_name = "strcasecmp_l"]
        fn native_strcasecmp_l(
            a: *const libc::c_char,
            b: *const libc::c_char,
            locale: libc::locale_t,
        ) -> libc::c_int;
    }
    let a = std::ffi::CString::new(a).ok()?;
    let b = std::ffi::CString::new(b).ok()?;
    // Query the C locale explicitly: ambient locale must not change the ASCII
    // comparison for a case-folding filesystem. Unknown names never reach here.
    let locale =
        unsafe { libc::newlocale(libc::LC_CTYPE_MASK, c"C".as_ptr(), std::ptr::null_mut()) };
    if locale.is_null() {
        return None;
    }
    let equal = unsafe { native_strcasecmp_l(a.as_ptr(), b.as_ptr(), locale) == 0 };
    unsafe { libc::freelocale(locale) };
    Some(equal)
}
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
fn known_namespace(_: &Dir) -> bool {
    false
}
#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios"
)))]
fn native_case_equal(_: &str, _: &str) -> Option<bool> {
    None
}
