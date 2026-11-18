use super::*;
use std::{io::Write, os::windows::io::AsRawHandle};
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationFile(
        file: *mut std::ffi::c_void,
        status: *mut [usize; 2],
        value: *mut u32,
        length: u32,
        class: u32,
    ) -> i32;
}
fn access(file: &std::fs::File) -> u32 {
    let mut status = [0usize; 2];
    let mut mask = 0;
    // SAFETY: live handle and exact FileAccessInformation output size.
    assert_eq!(
        unsafe { NtQueryInformationFile(file.as_raw_handle(), &mut status, &mut mask, 4, 8) },
        0
    );
    mask
}
#[test]
fn v2_windows_held_replacement_retains_expected_reader_and_original_delete_fd() {
    let f = Fixture::new();
    fs::write(f.root.join("user/document.txt"), b"before").unwrap();
    let (owner, path) = f.reserve();
    let mut working = owner.create(&|| Ok(())).unwrap();
    working.write_all(b"after").unwrap();
    working.sync_all().unwrap();
    let target = f.cap.target("document.txt").unwrap();
    let previous = crate::content::open_regular(&target.parent, &target.name).unwrap();
    let expected = HandleIdentity::from_cap_file(&previous).unwrap();
    crate::platform::preserve_permissions(&previous, &working).unwrap();
    {
        let capture = owner.capture.lock().unwrap();
        assert_ne!(access(capture.file.as_ref().unwrap()) & 0x10000, 0);
        assert_ne!(
            access(&working.try_clone().unwrap().into_std()) & 0x10000,
            0
        );
    }
    let mutation = FileService::mutation_lock().unwrap();
    f.journal()
        .lock()
        .unwrap()
        .phase(&path, crate::journal::StagePhase::Publishing, &mutation)
        .unwrap();
    owner.publishing().unwrap();
    owner.publish(true, &|| Ok(())).unwrap();
    owner.visible().unwrap();
    owner.published().unwrap();
    // Old FD is intentionally STILL live and points to the unchanged old object.
    assert_eq!(HandleIdentity::from_cap_file(&previous).unwrap(), expected);
    assert_ne!(
        HandleIdentity::from_cap_file(&previous).unwrap(),
        HandleIdentity::from_cap_file(&working).unwrap()
    );
    drop(mutation);
    drop(previous);
    drop(working);
    f.finish(&owner, &path);
    assert_eq!(
        fs::read(f.root.join("user/document.txt")).unwrap(),
        b"after"
    );
}
