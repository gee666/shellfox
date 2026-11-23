use crate::support::Fixture;
use shellfox_files::*;
use std::{
    fs,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        EqualSid, GetAce, GetSecurityDescriptorControl, ACCESS_ALLOWED_ACE,
        DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
    },
};

fn owner_only(file: &fs::File, inheritance: u8) {
    let mut acl = null_mut();
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: live read handle and initialized output storage. Descriptor owns ACL and owner.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(code, 0);
    let mut control = 0;
    let mut revision = 0;
    let mut ace = null_mut();
    // SAFETY: ACL, SID and descriptor are live until LocalFree and ACE index is checked by GetAce.
    unsafe {
        assert!(!acl.is_null());
        assert_ne!(
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
            0
        );
        assert_ne!(control & SE_DACL_PROTECTED, 0);
        // App directories also grant SYSTEM, as a second, non-inherited ACE.
        assert!((*acl).AceCount == 1 || (inheritance == 3 && (*acl).AceCount == 2));
        assert_ne!(GetAce(acl, 0, &mut ace), 0);
        let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
        assert_eq!(ace.Header.AceFlags, inheritance);
        assert_eq!(ace.Mask, 0x1f01ff);
        assert_ne!(
            EqualSid(owner, (&ace.SidStart as *const u32).cast_mut().cast()),
            0
        );
        LocalFree(descriptor);
    }
}

#[test]
fn every_stage_is_private_before_bytes_and_inbox_acl_never_inherits_broad_parent() {
    let f = Fixture::new();
    let mut registry = super::streams::ready(TransferLimits::default());
    let actor = super::streams::actor();
    let cap = super::streams::capability(&f);
    let start = registry
        .begin_write(
            &actor,
            cap,
            &super::streams::create("new", 1),
            &super::streams::allow,
        )
        .unwrap();
    let stage = fs::read_dir(&f.root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    owner_only(&fs::File::open(stage).unwrap(), 0);
    registry
        .write_chunk(
            &actor,
            &WriteChunk {
                transfer_id: start.transfer_id,
                offset: 0,
                data: vec![1],
            },
            &super::streams::allow,
        )
        .unwrap();
    registry
        .finish_write(
            &actor,
            &TransferIdentity {
                transfer_id: start.transfer_id,
            },
            &super::streams::allow,
        )
        .unwrap();
    owner_only(&fs::File::open(f.root.join("new")).unwrap(), 0);
    let inbox = UploadInbox::open(f.root.join("inbox"), UploadLimits::default()).unwrap();
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x02000000)
        .open(inbox.root_path())
        .unwrap();
    owner_only(&file, 3);
}
