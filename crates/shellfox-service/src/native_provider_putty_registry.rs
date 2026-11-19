use super::*;
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
    System::Registry::*,
};

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: this key is owned by the successful RegOpenKeyExW call.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
fn open(root: HKEY, name: &str) -> Result<Option<Key>> {
    let mut key = null_mut();
    // SAFETY: path is NUL terminated and output is a writable HKEY slot.
    let status = unsafe { RegOpenKeyExW(root, wide(name).as_ptr(), 0, KEY_READ, &mut key) };
    match status {
        ERROR_SUCCESS => Ok(Some(Key(key))),
        ERROR_FILE_NOT_FOUND => Ok(None),
        _ => Err(std::io::Error::from_raw_os_error(status as i32).into()),
    }
}

pub(super) fn read() -> Result<Entries> {
    let Some(root) = open(HKEY_CURRENT_USER, r"Software\SimonTatham\PuTTY\Sessions")? else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    let mut total = 0;
    for index in 0..=MAX_SESSIONS {
        let mut chars = [0; 256];
        let mut len = chars.len() as u32;
        // SAFETY: exact writable capacity supplied, optional fields are null.
        let status = unsafe {
            RegEnumKeyExW(
                root.0,
                index as u32,
                chars.as_mut_ptr(),
                &mut len,
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if index == MAX_SESSIONS {
            bail!("Too many PuTTY sessions.");
        }
        if status != ERROR_SUCCESS {
            return Err(std::io::Error::from_raw_os_error(status as i32).into());
        }
        let Ok(name) = String::from_utf16(&chars[..len as usize]) else {
            continue;
        };
        let Some(key) = open(root.0, &name)? else {
            continue;
        };
        let mut values = HashMap::new();
        total += name.len();
        for &field in FIELDS {
            if let Some(value) = read_value(&key, field) {
                total += field.len() + value.len();
                if total > MAX_BYTES {
                    bail!("PuTTY sessions exceed the 8 MiB import limit.");
                }
                values.insert(field.to_owned(), value);
            }
        }
        result.push((name, values));
    }
    Ok(result)
}

fn read_value(key: &Key, name: &str) -> Option<String> {
    let mut data = [0u16; 32768];
    let mut len = (data.len() * 2) as u32;
    let mut kind = 0;
    // SAFETY: buffer and lengths match. Only bounded REG_SZ/DWORD reads, no expansion.
    let status = unsafe {
        RegGetValueW(
            key.0,
            null(),
            wide(name).as_ptr(),
            RRF_RT_REG_SZ | RRF_RT_REG_DWORD,
            &mut kind,
            data.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    match kind {
        REG_SZ if len >= 2 && len.is_multiple_of(2) => {
            let text = &data[..len as usize / 2 - 1];
            if text.contains(&0) {
                return None;
            }
            String::from_utf16(text).ok()
        }
        REG_DWORD if len == 4 => Some((u32::from(data[0]) | u32::from(data[1]) << 16).to_string()),
        _ => None,
    }
}
