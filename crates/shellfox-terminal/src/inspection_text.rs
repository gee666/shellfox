//! Read text only through a process handle whose ownership/birth was verified.
use super::*;
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
use windows_sys::Win32::{Foundation::LocalFree, System::Threading::QueryFullProcessImageNameW};

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationProcess(
        process: HANDLE,
        class: u32,
        buffer: *mut std::ffi::c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn CommandLineToArgvW(command: *const u16, argc: *mut i32) -> *mut *mut u16;
}

pub(super) fn read(
    handle: HANDLE,
) -> Result<(OsString, OsString, Vec<OsString>), InspectionFailure> {
    let mut image = vec![0u16; 32768];
    let mut size = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, 0, image.as_mut_ptr(), &mut size) } == 0 {
        return Err(InspectionFailure::Unavailable);
    }
    if size == 0 || size as usize >= image.len() {
        return Err(InspectionFailure::Malformed);
    }
    let executable = OsString::from_wide(&image[..size as usize]);
    let process_name = PathBuf::from(&executable)
        .file_name()
        .ok_or(InspectionFailure::Malformed)?
        .to_os_string();
    let command = command_line(handle)?;
    let argv = arguments(&command)?;
    Ok((executable, process_name, argv))
}
fn command_line(handle: HANDLE) -> Result<Vec<u16>, InspectionFailure> {
    // ProcessCommandLineInformation copies an owned UNICODE_STRING into this
    // buffer. It requires only query access on supported Windows, not VM_READ.
    const CLASS: u32 = 60;
    let mut required = 0;
    unsafe {
        NtQueryInformationProcess(handle, CLASS, ptr::null_mut(), 0, &mut required);
    }
    let length = required as usize;
    if length < mem::size_of::<UnicodeString>()
        || length > MAX_INSPECTION_FIELD_BYTES + mem::size_of::<UnicodeString>() + 2
    {
        return Err(InspectionFailure::Bounds);
    }
    let mut storage = vec![0usize; length.div_ceil(mem::size_of::<usize>())];
    if unsafe {
        NtQueryInformationProcess(
            handle,
            CLASS,
            storage.as_mut_ptr().cast(),
            length as u32,
            &mut required,
        )
    } < 0
    {
        return Err(InspectionFailure::Unavailable);
    }
    let string = unsafe { &*storage.as_ptr().cast::<UnicodeString>() };
    let bytes = usize::from(string.length);
    let start = string.buffer as usize;
    let base = storage.as_ptr() as usize;
    let end = base
        .checked_add(length)
        .ok_or(InspectionFailure::Malformed)?;
    if bytes % 2 != 0
        || !start.is_multiple_of(mem::align_of::<u16>())
        || string.length > string.maximum_length
        || bytes > MAX_INSPECTION_FIELD_BYTES
        || start < base + mem::size_of::<UnicodeString>()
        || start.checked_add(bytes).is_none_or(|value| value > end)
    {
        return Err(InspectionFailure::Malformed);
    }
    let text = unsafe { std::slice::from_raw_parts(string.buffer, bytes / 2) };
    if text.contains(&0) {
        return Err(InspectionFailure::Malformed);
    }
    Ok(text.iter().copied().chain(Some(0)).collect())
}
fn arguments(command: &[u16]) -> Result<Vec<OsString>, InspectionFailure> {
    let mut count = 0;
    let pointers = unsafe { CommandLineToArgvW(command.as_ptr(), &mut count) };
    if pointers.is_null() {
        return Err(InspectionFailure::Unavailable);
    }
    struct Allocation(*mut *mut u16);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }
    let _allocation = Allocation(pointers);
    if !(0..=4096).contains(&count) {
        return Err(InspectionFailure::Bounds);
    }
    let mut argv = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let argument = unsafe { *pointers.add(index) };
        if argument.is_null() {
            return Err(InspectionFailure::Malformed);
        }
        let mut len = 0;
        while len <= MAX_INSPECTION_FIELD_BYTES / 2 && unsafe { *argument.add(len) } != 0 {
            len += 1;
        }
        if len > MAX_INSPECTION_FIELD_BYTES / 2 {
            return Err(InspectionFailure::Bounds);
        }
        argv.push(OsString::from_wide(unsafe {
            std::slice::from_raw_parts(argument, len)
        }));
    }
    Ok(argv)
}
