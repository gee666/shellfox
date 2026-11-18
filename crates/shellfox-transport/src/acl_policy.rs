#[cfg(any(target_os = "macos", test))]
pub(crate) fn darwin_acl_absent(data: &[u8]) -> bool {
    fn word(data: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_ne_bytes(data.get(at..at + 4)?.try_into().ok()?))
    }
    let check = || -> Option<bool> {
        let size = usize::try_from(word(data, 0)?).ok()?;
        if size < 32 || size > data.len() {
            return Some(false);
        }
        let common = word(data, 4)?;
        if common & 0x00400000 == 0 || common & !(0x00400000 | 0x80000000) != 0 {
            return Some(false);
        }
        for offset in [8, 12, 16, 20] {
            if word(data, offset)? != 0 {
                return Some(false);
            }
        }
        let relative = i32::from_ne_bytes(word(data, 24)?.to_ne_bytes());
        let start = usize::try_from(24i64 + i64::from(relative)).ok()?;
        let length = usize::try_from(word(data, 28)?).ok()?;
        if start > size || start.checked_add(length)? > size {
            return Some(false);
        }
        if length == 0 {
            return Some(true);
        }
        if length < 44 || word(data, start)? != 0x012cc16d {
            return Some(false);
        }
        let entries = word(data, start + 36)?;
        Some(entries == 0 || entries == u32::MAX)
    };
    check().unwrap_or(false)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn set(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_ne_bytes());
    }
    #[test]
    fn pure_darwin_acl_policy_requires_native_security_reply_and_no_entries() {
        let mut data = vec![0; 76];
        set(&mut data, 0, 76);
        set(&mut data, 4, 0x80400000);
        set(&mut data, 24, 8);
        set(&mut data, 28, 44);
        set(&mut data, 32, 0x012cc16d);
        assert!(darwin_acl_absent(&data));
        set(&mut data, 68, 1);
        assert!(!darwin_acl_absent(&data)); // deny-only/inherited are not guessed safe
        set(&mut data, 68, u32::MAX);
        assert!(darwin_acl_absent(&data));
        set(&mut data, 4, 0x80000000);
        assert!(!darwin_acl_absent(&data));
        set(&mut data, 4, 0x80400000);
        set(&mut data, 24, i32::MIN.cast_unsigned());
        assert!(!darwin_acl_absent(&data));
        assert!(!darwin_acl_absent(&data[..20]));
    }
}
