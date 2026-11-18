//! Same-user proof for both ends of the local pipe, in addition to its DACL.
use anyhow::{bail, Result};
use std::{os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{EqualSid, GetTokenInformation, RevertToSelf, TokenUser, TOKEN_QUERY, TOKEN_USER},
    System::{
        Pipes::{GetNamedPipeServerProcessId, ImpersonateNamedPipeClient},
        Threading::{
            GetCurrentProcess, GetCurrentThread, OpenProcess, OpenProcessToken, OpenThreadToken,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};
struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Revert;
impl Drop for Revert {
    fn drop(&mut self) {
        // Continuing under a client's impersonation token would cross the
        // privilege boundary. Windows recommends termination if reversion fails.
        if unsafe { RevertToSelf() } == 0 {
            std::process::abort();
        }
    }
}

pub(crate) fn verify_client(pipe: &tokio::net::windows::named_pipe::NamedPipeServer) -> Result<()> {
    // Called after reading one frame, as required by ImpersonateNamedPipeClient.
    let own = process_token(unsafe { GetCurrentProcess() })?;
    if unsafe { ImpersonateNamedPipeClient(pipe.as_raw_handle()) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let revert = Revert;
    let mut token = ptr::null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let remote = OwnedHandle(token);
    let result = same_user(own.0, remote.0);
    drop(revert);
    result
}
pub(crate) fn verify_server(pipe: &tokio::net::windows::named_pipe::NamedPipeClient) -> Result<()> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let process = OwnedHandle(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
    if process.0.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let remote = process_token(process.0)?;
    let own = process_token(unsafe { GetCurrentProcess() })?;
    same_user(own.0, remote.0)
}
fn process_token(process: HANDLE) -> Result<OwnedHandle> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(OwnedHandle(token))
}
fn user(token: HANDLE) -> Result<Vec<usize>> {
    let mut length = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length);
    }
    if length == 0 || length > 65536 {
        bail!("Invalid local peer token length.");
    }
    // usize storage provides sufficient TOKEN_USER pointer alignment.
    let mut storage = vec![0_usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(storage)
}
fn same_user(first: HANDLE, second: HANDLE) -> Result<()> {
    let a = user(first)?;
    let b = user(second)?;
    let a = unsafe { &*a.as_ptr().cast::<TOKEN_USER>() };
    let b = unsafe { &*b.as_ptr().cast::<TOKEN_USER>() };
    if unsafe { EqualSid(a.User.Sid, b.User.Sid) } == 0 {
        bail!("Local pipe peer is not the current user.");
    }
    Ok(())
}
