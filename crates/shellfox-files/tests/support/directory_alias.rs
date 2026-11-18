use super::*;
#[cfg(windows)]
#[test]
#[ignore = "requires owned filesystem with native8.3 alternate names enabled; never changes volume policy"]
fn existing_windows_alternate_spelling_uses_exact_native_directory_identity() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;
    let fixture = Fixture::new();
    let long = fixture.root.join("owned long directory spelling");
    std::fs::create_dir(&long).unwrap();
    let path: Vec<u16> = long.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut output = vec![0_u16; 32768];
    let count =
        unsafe { GetShortPathNameW(path.as_ptr(), output.as_mut_ptr(), output.len() as u32) };
    assert!(
        count > 0 && (count as usize) < output.len(),
        "native8.3 query prerequisite unavailable"
    );
    let short = std::path::PathBuf::from(std::ffi::OsString::from_wide(&output[..count as usize]));
    assert_ne!(
        short, long,
        "filesystem does not expose an alternate spelling; prerequisite not met"
    );
    let cap = Arc::new(FileService::local_desktop(Access::ReadOnly));
    let a = cap.plan_directory(&location(&long)).unwrap();
    let b = cap.plan_directory(&location(&short)).unwrap();
    assert_eq!(a.relation_to(&b).unwrap(), R::Same);
    let child = long.join("new-child");
    let alias = short.join("new-child");
    let a = cap.plan_directory(&location(&child)).unwrap();
    let b = cap.plan_directory(&location(&alias)).unwrap();
    assert_eq!(a.relation_to(&b).unwrap(), R::Same);
    assert!(!child.exists());
}
