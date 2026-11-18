use super::{PowerError, PowerLease};
use std::{
    ffi::{c_char, c_void},
    ptr,
};

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        text: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
}
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        kind: *const c_void,
        level: u32,
        name: *const c_void,
        id: *mut u32,
    ) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}
const UTF8: u32 = 0x0800_0100;
const ON: u32 = 255;

pub struct Lease {
    display: Option<u32>,
    system: Option<u32>,
    partial_reason: String,
}

fn assertion(kind: &'static [u8]) -> Result<u32, PowerError> {
    // Both strings are static NUL-terminated UTF-8; the returned CF objects are released.
    unsafe {
        let ty = CFStringCreateWithCString(ptr::null(), kind.as_ptr().cast(), UTF8);
        if ty.is_null() {
            return Err(PowerError("Could not allocate assertion type".into()));
        }
        let name = CFStringCreateWithCString(ptr::null(), c"Shellfox keep awake".as_ptr(), UTF8);
        if name.is_null() {
            CFRelease(ty);
            return Err(PowerError("Could not allocate assertion name".into()));
        }
        let mut id = 0;
        let result = IOPMAssertionCreateWithName(ty, ON, name, &mut id);
        CFRelease(name);
        CFRelease(ty);
        if result != 0 {
            return Err(PowerError::operation("IOPMAssertionCreateWithName", result));
        }
        Ok(id)
    }
}
pub fn acquire() -> Result<Lease, PowerError> {
    let mut lease = Lease {
        display: None,
        system: None,
        partial_reason: "macOS explicit system sleep, disk idle and user-activity assertions are not implemented.".into(),
    };
    lease.system = Some(assertion(b"PreventUserIdleSystemSleep\0")?);
    match assertion(b"PreventUserIdleDisplaySleep\0") {
        Ok(display) => lease.display = Some(display),
        Err(error) => {
            lease.partial_reason = format!(
                "Display assertion unavailable: {error}; {}",
                lease.partial_reason
            )
        }
    }
    Ok(lease)
}
fn clear(id: &mut Option<u32>) -> Result<(), PowerError> {
    if let Some(value) = *id {
        // The ID belongs to this lease and is released at most once successfully.
        let result = unsafe { IOPMAssertionRelease(value) };
        if result != 0 {
            return Err(PowerError::operation("IOPMAssertionRelease", result));
        }
        *id = None;
    }
    Ok(())
}
impl PowerLease for Lease {
    fn partial_reason(&self) -> Option<&str> {
        Some(&self.partial_reason)
    }
    fn release(&mut self) -> Result<(), PowerError> {
        clear(&mut self.display)?;
        clear(&mut self.system)
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
