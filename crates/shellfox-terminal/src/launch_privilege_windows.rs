use super::*;
use std::{
    mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{Foundation::HANDLE, Security::*, System::Threading::OpenProcessToken};
pub(super) struct InitialProof {
    _birth: crate::inspection::RawProcessIdentity,
}
pub(super) fn capture(
    process: HANDLE,
    job: HANDLE,
) -> Result<(WindowsLaunchToken, InitialProof), LaunchUnknownReason> {
    // Borrow only. Extra Job HANDLEs would extend KILL_ON_JOB_CLOSE lifetime.
    let identity = crate::inspection::verify_member(job, process)
        .map_err(|_| LaunchUnknownReason::OwnershipUncertain)?;
    let token = read_token(process)?;
    if crate::inspection::verify_member(job, process)
        .map_err(|_| LaunchUnknownReason::OwnershipUncertain)?
        != identity
    {
        return Err(LaunchUnknownReason::OwnershipUncertain);
    }
    Ok((token, InitialProof { _birth: identity }))
}
fn scalar(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<u32, LaunchUnknownReason> {
    let mut value = 0u32;
    let mut returned = 0;
    if unsafe {
        GetTokenInformation(
            token,
            class,
            (&mut value as *mut u32).cast(),
            mem::size_of::<u32>() as u32,
            &mut returned,
        )
    } == 0
        || returned != 4
    {
        return Err(LaunchUnknownReason::TokenQueryUnavailable);
    }
    Ok(value)
}
fn integrity(token: HANDLE) -> Result<u32, LaunchUnknownReason> {
    let mut storage = [0usize; 512];
    let mut returned = 0;
    if unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            storage.as_mut_ptr().cast(),
            mem::size_of_val(&storage) as u32,
            &mut returned,
        )
    } == 0
        || (returned as usize) < mem::size_of::<TOKEN_MANDATORY_LABEL>()
        || returned as usize > mem::size_of_val(&storage)
    {
        return Err(LaunchUnknownReason::TokenQueryUnavailable);
    }
    let label = unsafe { &*storage.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let sid = label.Label.Sid;
    let start = sid as usize;
    let base = storage.as_ptr() as usize;
    let end = base + returned as usize;
    if start < base
        || !start.is_multiple_of(mem::align_of::<u32>())
        || start.checked_add(8).is_none_or(|value| value > end)
    {
        return Err(LaunchUnknownReason::TokenQueryUnavailable);
    }
    let count = unsafe { *(sid.cast::<u8>().add(1)) } as usize;
    if count == 0
        || count > 15
        || start
            .checked_add(8 + count * 4)
            .is_none_or(|value| value > end)
        || unsafe { IsValidSid(sid) } == 0
    {
        return Err(LaunchUnknownReason::TokenQueryUnavailable);
    }
    Ok(unsafe { *GetSidSubAuthority(sid, (count - 1) as u32) })
}
pub(super) fn read_token(process: HANDLE) -> Result<WindowsLaunchToken, LaunchUnknownReason> {
    let mut handle = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) } == 0 || handle.is_null() {
        return Err(LaunchUnknownReason::TokenQueryUnavailable);
    }
    let token = unsafe { OwnedHandle::from_raw_handle(handle.cast()) };
    let token = token.as_raw_handle().cast();
    let elevated = scalar(token, TokenElevation)?;
    let ui_access = scalar(token, TokenUIAccess)?;
    if elevated > 1 || ui_access > 1 {
        return Err(LaunchUnknownReason::InconsistentToken);
    }
    let elevation_type = match scalar(token, TokenElevationType)? as i32 {
        value if value == TokenElevationTypeDefault => WindowsElevationType::Default,
        value if value == TokenElevationTypeFull => WindowsElevationType::Full,
        value if value == TokenElevationTypeLimited => WindowsElevationType::Limited,
        _ => return Err(LaunchUnknownReason::InconsistentToken),
    };
    Ok(WindowsLaunchToken {
        elevated: elevated != 0,
        elevation_type,
        integrity_rid: integrity(token)?,
        ui_access: ui_access != 0,
    })
}
