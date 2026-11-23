use std::{
    ffi::c_void,
    fs,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn DeviceIoControl(
        file: *mut c_void,
        code: u32,
        input: *const c_void,
        input_size: u32,
        output: *mut c_void,
        output_size: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
}

pub fn create(from: &Path, to: &Path) {
    fs::create_dir(to).unwrap();
    let file = fs::OpenOptions::new()
        .access_mode(0x40000000)
        .custom_flags(0x00200000 | 0x02000000)
        .open(to)
        .unwrap();
    let substitute: Vec<u16> = std::ffi::OsStr::new(&format!("\\??\\{}", from.display()))
        .encode_wide()
        .collect();
    let print: Vec<u16> = from.as_os_str().encode_wide().collect();
    let data_len = 8 + (substitute.len() + print.len() + 2) * 2;
    let mut data = Vec::with_capacity(data_len + 8);
    data.extend_from_slice(&0xa0000003u32.to_le_bytes());
    data.extend_from_slice(&(data_len as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    data.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
    data.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    for value in substitute.into_iter().chain([0]).chain(print).chain([0]) {
        data.extend_from_slice(&value.to_le_bytes());
    }
    let mut returned = 0;
    // SAFETY: this live directory handle and bounded buffer describe one mount-point reparse.
    let result = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            0x900a4,
            data.as_ptr().cast(),
            data.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    assert_ne!(result, 0, "{}", std::io::Error::last_os_error());
}
