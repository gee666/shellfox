use super::*;
use std::{fs, os::windows::io::AsRawHandle, ptr::null_mut};
use windows_sys::Win32::Security::{
    AddAccessAllowedAceEx,
    Authorization::{SetSecurityInfo, SE_FILE_OBJECT},
    CreateWellKnownSid, InitializeAcl, WinWorldSid, ACL, DACL_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION,
};
pub(super) struct Fixture {
    pub path: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        use std::os::windows::fs::OpenOptionsExt;
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = base.to_str().unwrap().trim_start_matches("\\\\?\\");
        let path = Path::new(base).join(format!("rust-win-anchor-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join("parent")).unwrap();
        let parent = fs::OpenOptions::new()
            .access_mode(0x20000 | 0x40000)
            .custom_flags(0x02000000)
            .open(path.join("parent"))
            .unwrap();
        let owner = crate::private_security::current_sid().unwrap();
        let mut world = [0u64; 9];
        let mut length = 72;
        let mut acl = [0u64; 64];
        // SAFETY: fixed-size aligned SID/ACL storage; only this owned temporary parent is modified.
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
            assert_ne!(InitializeAcl(acl.as_mut_ptr().cast::<ACL>(), 512, 2), 0);
            assert_ne!(
                AddAccessAllowedAceEx(
                    acl.as_mut_ptr().cast(),
                    2,
                    3,
                    0x1f01ff,
                    owner.as_ptr().cast_mut().cast()
                ),
                0
            );
            assert_ne!(
                AddAccessAllowedAceEx(
                    acl.as_mut_ptr().cast(),
                    2,
                    3,
                    0x001301bf,
                    world.as_mut_ptr().cast()
                ),
                0
            );
            assert_eq!(
                SetSecurityInfo(
                    parent.as_raw_handle(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    acl.as_ptr().cast(),
                    null_mut()
                ),
                0
            );
        }
        Self { path }
    }
    pub fn plan(&self) -> (Arc<FileService>, DirectoryPlan) {
        let authority =
            Arc::new(FileService::approved_root(&self.path, crate::Access::ReadWrite).unwrap());
        let plan = authority
            .plan_directory(&Location {
                path: "parent/private-child".into(),
            })
            .unwrap();
        (authority, plan)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}
pub(super) fn parent(plan: &DirectoryPlan) -> std::fs::File {
    plan.original
        .dirs
        .last()
        .unwrap()
        .try_clone()
        .unwrap()
        .into_std_file()
}
pub(super) fn delete_open(parent: &std::fs::File) -> Result<std::fs::File> {
    crate::private_windows::open(
        parent,
        std::ffi::OsStr::new("private-child"),
        PrivateKind::Directory,
        crate::PrivateAccess::ReadOnly,
        None,
        Some(0x110080),
        None,
    )
}
pub(super) fn denied_delete(parent: &std::fs::File) {
    let error = delete_open(parent).unwrap_err();
    assert!(
        error.message.contains("NTSTATUS 0xc0000043"),
        "{}",
        error.message
    );
}
