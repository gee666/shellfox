use super::{PowerError, PowerLease};
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    System::{
        Power::{
            PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired,
            PowerRequestSystemRequired, PowerSetRequest,
        },
        Threading::{POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0},
    },
};

pub struct Lease {
    handle: OwnedHandle,
    display: bool,
    system: bool,
}

pub fn acquire() -> Result<Lease, PowerError> {
    let mut reason: Vec<u16> = "Shellfox keep awake\0".encode_utf16().collect();
    let context = REASON_CONTEXT {
        Version: 0,
        Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
        Reason: REASON_CONTEXT_0 {
            SimpleReasonString: reason.as_mut_ptr(),
        },
    };
    // The context and string remain valid for the duration of the call.
    let raw = unsafe { PowerCreateRequest(&context) };
    if raw == INVALID_HANDLE_VALUE || raw.is_null() {
        return Err(PowerError::operation(
            "PowerCreateRequest",
            io::Error::last_os_error(),
        ));
    }
    // PowerCreateRequest returns a new owned kernel handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut lease = Lease {
        handle,
        display: false,
        system: false,
    };
    if unsafe { PowerSetRequest(lease.handle.as_raw_handle(), PowerRequestSystemRequired) } == 0 {
        return Err(PowerError::operation(
            "PowerSetRequest(system)",
            io::Error::last_os_error(),
        ));
    }
    lease.system = true;
    if unsafe { PowerSetRequest(lease.handle.as_raw_handle(), PowerRequestDisplayRequired) } == 0 {
        return Err(PowerError::operation(
            "PowerSetRequest(display)",
            io::Error::last_os_error(),
        ));
    }
    lease.display = true;
    Ok(lease)
}
impl PowerLease for Lease {
    fn partial_reason(&self) -> Option<&str> {
        Some("Windows lid policy, crash recovery and idle-input helper are not implemented.")
    }
    fn release(&mut self) -> Result<(), PowerError> {
        if self.display {
            if unsafe {
                PowerClearRequest(self.handle.as_raw_handle(), PowerRequestDisplayRequired)
            } == 0
            {
                return Err(PowerError::operation(
                    "PowerClearRequest(display)",
                    io::Error::last_os_error(),
                ));
            }
            self.display = false;
        }
        if self.system {
            if unsafe { PowerClearRequest(self.handle.as_raw_handle(), PowerRequestSystemRequired) }
                == 0
            {
                return Err(PowerError::operation(
                    "PowerClearRequest(system)",
                    io::Error::last_os_error(),
                ));
            }
            self.system = false;
        }
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
