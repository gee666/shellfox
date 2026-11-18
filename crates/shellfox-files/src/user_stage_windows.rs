//! One retained native attempt for a scoped user stage. Not a public File-based authority API.
use crate::{ErrorCode, FileError, Result};
use std::{
    ffi::{c_void, OsStr},
    fs::File,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
};
#[repr(C)]
struct Name {
    length: u16,
    maximum: u16,
    buffer: usize,
}
#[repr(C)]
struct Attributes {
    length: u32,
    root: usize,
    name: usize,
    flags: u32,
    security: usize,
    qos: usize,
}
// Native pointers are offsets/addresses into this stable Box and its owned vectors. They are
// never dereferenced by Rust outside the serialized native call. All inputs outlive uncertainty.
struct Input {
    parent: File,
    leaf: Vec<u16>,
    security: Vec<usize>,
    name: Name,
    attributes: Attributes,
    handle: usize,
    status: [usize; 2],
}
const _: () = assert!(size_of::<Attributes>() == size_of::<crate::private_windows::Attributes>());
const _: () =
    assert!(size_of::<Name>() == size_of::<windows_sys::Win32::Foundation::UNICODE_STRING>());
pub(crate) struct StageAttempt {
    input: Option<Box<Input>>,
    entered: bool,
}
impl StageAttempt {
    pub fn prepare(parent: File, leaf: &OsStr) -> Result<Self> {
        use windows_sys::Win32::Security::MakeSelfRelativeSD;
        let descriptor = crate::private_security::Security::new(false)?;
        let mut bytes = 0u32;
        // SAFETY: size query only; existing protected descriptor owns every SID/ACL pointer.
        unsafe {
            MakeSelfRelativeSD(descriptor.as_ptr(), std::ptr::null_mut(), &mut bytes);
        }
        if bytes == 0 || bytes > 65536 {
            return Err(FileError::denied());
        }
        let mut security = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        // SAFETY: aligned destination has the exact queried capacity. Self-relative SD has no external pointers.
        if unsafe {
            MakeSelfRelativeSD(
                descriptor.as_ptr(),
                security.as_mut_ptr().cast(),
                &mut bytes,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let leaf = leaf.encode_wide().collect::<Vec<_>>();
        let length = u16::try_from(leaf.len() * 2).map_err(|_| FileError::denied())?;
        let mut input = Box::new(Input {
            parent,
            leaf,
            security,
            name: Name {
                length,
                maximum: length,
                buffer: 0,
            },
            attributes: Attributes {
                length: size_of::<Attributes>() as u32,
                root: 0,
                name: 0,
                flags: 0x1000,
                security: 0,
                qos: 0,
            },
            handle: 0,
            status: [0; 2],
        });
        input.name.buffer = input.leaf.as_mut_ptr() as usize;
        input.attributes.root = input.parent.as_raw_handle() as usize;
        input.attributes.name = (&mut input.name as *mut Name) as usize;
        input.attributes.security = input.security.as_mut_ptr() as usize;
        Ok(Self {
            input: Some(input),
            entered: false,
        })
    }
    pub fn create_into(
        &mut self,
        slot: &mut Option<File>,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<()> {
        if slot.is_some() {
            return Err(FileError::denied());
        }
        let input = self.input.as_mut().ok_or_else(FileError::denied)?;
        // SAFETY: SAME persistent owner's stable Box contains outputs and ALL input buffers
        // before entry. FILE_CREATE, explicit protected SD, non-directory/no-reparse, sync I/O.
        check()?;
        self.entered = true;
        let code = unsafe {
            crate::private_windows::NtCreateFile(
                (&mut input.handle as *mut usize).cast(),
                0x1f01ff,
                (&input.attributes as *const Attributes).cast(),
                &mut input.status,
                std::ptr::null(),
                0x80,
                3,
                2,
                0x20 | 0x200000 | 0x40,
                std::ptr::null(),
                0,
            )
        };
        if code != 0 {
            // Do not read/adopt possibly asynchronous output, free buffers, close a guessed
            // handle or infer no effect. The original attempt stays in the journal owner.
            return Err(FileError::new(ErrorCode::StorageFailed,"Native stage outcome is uncertain; original outputs and inputs retained. No retry was performed."));
        }
        if input.handle == 0 || input.handle == usize::MAX {
            return Err(FileError::new(
                ErrorCode::StorageFailed,
                "Native stage returned no valid owned handle; original attempt retained.",
            ));
        }
        if input.status[0] != 0 || input.status[1] != 2 {
            // Raw output was captured in this SAME persistent Box by the syscall. Do not
            // adopt/close it or free native inputs without the complete SUCCESS/CREATED tuple.
            return Err(FileError::new(ErrorCode::StorageFailed,"Native completion/creation evidence is unproven; original raw output and inputs retained."));
        }
        // Completed SUCCESS/CREATED owns this actual handle. Install it before every fallible
        // identity/permission/clone/authorization postcheck. No by-value creator return gap.
        *slot = Some(unsafe { File::from_raw_handle(input.handle as *mut c_void) });
        input.handle = 0;
        self.input = None; // positive completion tuple, native storage is no longer OS-live
        Ok(())
    }
}
impl Drop for StageAttempt {
    fn drop(&mut self) {
        if let Some(input) = self.input.take() {
            if !self.entered {
                return;
            }
            // Unknown completion is not close/free authority. At most JOURNAL_ENTRIES attempts
            // are admitted; keep possibly OS-live buffers/parent/raw output for process lifetime.
            let _ = Box::leak(input);
        }
    }
}
