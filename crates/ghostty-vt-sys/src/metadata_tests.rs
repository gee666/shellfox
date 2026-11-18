use super::*;

unsafe extern "C" {
    fn sf_metadata_title(input: *const u8, len: usize, out: *mut u8, out_len: *mut usize) -> bool;
    fn sf_metadata_cwd(input: *const u8, len: usize) -> bool;
}
fn title(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = [0; MAX_METADATA_TITLE_BYTES];
    let mut len = 0;
    // SAFETY: the C helper's output is bounded by the fixed title capacity.
    let valid =
        unsafe { sf_metadata_title(input.as_ptr(), input.len(), out.as_mut_ptr(), &mut len) };
    assert!(len <= out.len());
    valid.then(|| out[..len].to_vec())
}
fn cwd(input: &[u8]) -> bool {
    // SAFETY: a live borrowed input of exactly the supplied length, never retained.
    unsafe { sf_metadata_cwd(input.as_ptr(), input.len()) }
}

#[test]
fn c_title_validator_rejects_invalid_utf8_and_raw_overflow() {
    for invalid in [
        &[0xff][..],
        &[0x80],
        &[0xc0, 0x80],
        &[0xe0, 0x80, 0x80],
        &[0xed, 0xa0, 0x80],
        &[0xf4, 0x90, 0x80, 0x80],
        &[0xf0, 0x9f],
        &[b'a', 0xc2, b'b'],
    ] {
        assert_eq!(title(invalid), None);
    }
    assert_eq!(
        title(&vec![b'a'; MAX_RAW_TITLE_BYTES]),
        Some(vec![b'a'; MAX_METADATA_TITLE_UNITS])
    );
    assert_eq!(title(&vec![b'a'; MAX_RAW_TITLE_BYTES + 1]), None);
}

#[test]
fn c_title_validator_matches_utf16_and_unicode_trim_without_split_scalars() {
    assert_eq!(
        title("\u{feff} \u{85}A\u{85}B \u{3000}".as_bytes()),
        Some(b"AB".to_vec())
    );
    assert_eq!(
        title("界".repeat(200).as_bytes()),
        Some("界".repeat(200).into_bytes())
    );
    assert_eq!(
        title("🙂".repeat(150).as_bytes()),
        Some("🙂".repeat(100).into_bytes())
    );
    assert_eq!(
        title(format!("{}🙂", "a".repeat(199)).as_bytes()),
        Some(vec![b'a'; 199])
    );
    assert_eq!(title(b"\0\t\r\n"), Some(Vec::new()));
}

#[test]
fn c_cwd_validator_enforces_exact_byte_bound_without_decoding_a_path() {
    let maximum = format!("file:///{}", "a".repeat(MAX_METADATA_CWD_BYTES - 8));
    assert_eq!(maximum.len(), MAX_METADATA_CWD_BYTES);
    assert!(cwd(maximum.as_bytes()));
    assert!(!cwd(format!("{maximum}a").as_bytes()));
    assert!(cwd(b""));
    for bad in [
        b"file:///a%00".as_slice(),
        b"file:///a%80",
        b"file:///a%ed%a0%80",
        b"file:///a\xff",
        b"file:///a%C2%9f",
    ] {
        assert!(!cwd(bad));
    }
    assert!(cwd(b"file:///a%20b%3fc%23d"));
}

#[test]
fn rejected_c_title_never_returns_partial_output() {
    let mut out = [0xa5; MAX_METADATA_TITLE_BYTES];
    let mut len = 7;
    // SAFETY: bounded test input and correctly sized live output storage.
    let valid = unsafe { sf_metadata_title(b"good\xff".as_ptr(), 5, out.as_mut_ptr(), &mut len) };
    assert!(!valid);
    assert_eq!(len, 7);
    assert!(out.iter().all(|&byte| byte == 0xa5));
}

#[test]
fn owned_copy_checks_length_null_utf8_and_control_before_ack() {
    assert_eq!(copy_text(std::ptr::null(), 0, 1), Ok(None));
    assert_eq!(copy_text(std::ptr::null(), 1, 1), Err(-2));
    assert_eq!(copy_text(std::ptr::null(), 2, 1), Err(-6));
    assert_eq!(copy_text(b"\xff".as_ptr(), 1, 1), Err(-2));
    assert_eq!(copy_text(b"\n".as_ptr(), 1, 1), Err(-2));
    assert_eq!(copy_text(b"ok".as_ptr(), 2, 2), Ok(Some("ok".into())));
}
