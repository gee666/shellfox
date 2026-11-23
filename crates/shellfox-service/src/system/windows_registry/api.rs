use super::super::{
    error,
    registry::{RegistryStringKind, RegistryText},
    SystemResult,
};
use super::wide;
use shellfox_protocol::ErrorCode;
use std::ffi::c_void;

pub(super) type Key = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(root: Key, name: *const u16, options: u32, access: u32, key: *mut Key) -> i32;
    fn RegCreateKeyExW(
        root: Key,
        name: *const u16,
        reserved: u32,
        class: *mut u16,
        options: u32,
        access: u32,
        security: *mut c_void,
        key: *mut Key,
        disposition: *mut u32,
    ) -> i32;
    fn RegQueryValueExW(
        key: Key,
        name: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        size: *mut u32,
    ) -> i32;
    fn RegSetValueExW(
        key: Key,
        name: *const u16,
        reserved: u32,
        kind: u32,
        data: *const u8,
        size: u32,
    ) -> i32;
    pub(super) fn RegDeleteValueW(key: Key, name: *const u16) -> i32;
    pub(super) fn RegDeleteTreeW(key: Key, name: *const u16) -> i32;
    fn RegCloseKey(key: Key) -> i32;
    fn RegQueryInfoKeyW(
        key: Key,
        class: *mut u16,
        class_len: *mut u32,
        reserved: *mut u32,
        subkeys: *mut u32,
        max_subkey: *mut u32,
        max_class: *mut u32,
        values: *mut u32,
        max_name: *mut u32,
        max_data: *mut u32,
        security: *mut u32,
        last: *mut FileTime,
    ) -> i32;
}
pub(super) struct Handle(pub Key);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
pub(super) fn open(root: Key, name: &str, access: u32) -> SystemResult<Option<Handle>> {
    let mut key = std::ptr::null_mut();
    let name = wide(name);
    // SAFETY: fixed host-owned key names and initialized output pointer.
    match unsafe { RegOpenKeyExW(root, name.as_ptr(), 8, access | 0x100, &mut key) } {
        0 => {
            let handle = Handle(key);
            let link = wide("SymbolicLinkValue");
            let (mut kind, mut size) = (0, 0);
            // SAFETY: query only; registry symbolic links are not followed.
            let status = unsafe {
                RegQueryValueExW(
                    handle.0,
                    link.as_ptr(),
                    std::ptr::null_mut(),
                    &mut kind,
                    std::ptr::null_mut(),
                    &mut size,
                )
            };
            if status == 0 && kind == 6 {
                return Err(unavailable(
                    "Registry symbolic-link keys are not supported.",
                ));
            }
            Ok(Some(handle))
        }
        2 => Ok(None),
        _ => Err(unavailable(
            "System registry cannot be read with this account.",
        )),
    }
}
pub(super) fn info(key: Key) -> SystemResult<(u64, u32, u32)> {
    let (mut time, mut subkeys, mut values) = (FileTime::default(), 0, 0);
    // SAFETY: valid handle; optional outputs are null; supplied outputs live.
    if unsafe {
        RegQueryInfoKeyW(
            key,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut subkeys,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut values,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut time,
        )
    } != 0
    {
        return Err(unavailable("Registry revision cannot be queried."));
    }
    Ok((
        (u64::from(time.high) << 32) | u64::from(time.low),
        subkeys,
        values,
    ))
}
pub(super) fn value(key: Key, name: &str) -> SystemResult<Option<RegistryText>> {
    let name = wide(name);
    let (mut kind, mut size) = (0, 0);
    // SAFETY: read-only length query on a fixed value name.
    let result = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if result == 2 {
        return Ok(None);
    }
    if result != 0 || !matches!(kind, 1 | 2) || size > 65536 || size % 2 != 0 {
        return Err(unavailable("Unsupported or oversized registry string."));
    }
    let mut bytes = vec![0u16; (size as usize / 2) + 1];
    let mut actual = size;
    // SAFETY: allocated UTF-16 storage has at least actual bytes of capacity.
    if unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut kind,
            bytes.as_mut_ptr().cast(),
            &mut actual,
        )
    } != 0
        || actual > size
        || actual % 2 != 0
        || !matches!(kind, 1 | 2)
    {
        return Err(conflict());
    }
    bytes.truncate(actual as usize / 2);
    if bytes.last() == Some(&0) {
        bytes.pop();
    }
    if bytes.contains(&0) {
        return Err(unavailable("Embedded registry string terminator refused."));
    }
    let value =
        String::from_utf16(&bytes).map_err(|_| unavailable("Invalid registry string encoding."))?;
    Ok(Some(RegistryText {
        value,
        kind: if kind == 2 {
            RegistryStringKind::Expandable
        } else {
            RegistryStringKind::Plain
        },
    }))
}
pub(super) fn create(root: Key, name: &str, must_be_new: bool) -> SystemResult<Handle> {
    let name = wide(name);
    let (mut key, mut disposition) = (std::ptr::null_mut(), 0);
    // SAFETY: fixed host-generated key path; outputs initialized by Win32.
    if unsafe {
        RegCreateKeyExW(
            root,
            name.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            1 | 2 | 4 | 0x100,
            std::ptr::null_mut(),
            &mut key,
            &mut disposition,
        )
    } != 0
    {
        return Err(unavailable("Registry integration key cannot be created."));
    }
    let handle = Handle(key);
    if must_be_new && disposition != 1 {
        return Err(conflict());
    }
    Ok(handle)
}
pub(super) fn set(key: Key, name: &str, value: &RegistryText) -> SystemResult<()> {
    if value.value.len() > 32767 || value.value.contains('\0') {
        return Err(unavailable("Registry output string exceeds bound."));
    }
    let name = wide(name);
    let data = wide(&value.value);
    // SAFETY: live UTF-16 buffers; byte length includes the final terminator.
    if unsafe {
        RegSetValueExW(
            key,
            name.as_ptr(),
            0,
            if value.kind == RegistryStringKind::Expandable {
                2
            } else {
                1
            },
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    } != 0
    {
        return Err(unavailable("Registry integration update failed."));
    }
    Ok(())
}
pub(super) fn unavailable(message: &str) -> shellfox_protocol::AppError {
    error(ErrorCode::NativeUnavailable, message)
}
pub(super) fn conflict() -> shellfox_protocol::AppError {
    error(
        ErrorCode::FileConflict,
        "Registry state changed since query; no automatic retry.",
    )
}
