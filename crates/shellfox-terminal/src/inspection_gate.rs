//! Private startup proof for the exact original command child.
use crate::{
    inspection::{self, InspectionCapability, RawProcessIdentity},
    windows::{wide, Job},
};
use anyhow::{ensure, Context, Result};
use std::{
    ffi::{OsStr, OsString},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING,
        PIPE_ACCESS_DUPLEX,
    },
    System::{
        Pipes::*,
        Threading::{GetCurrentProcess, GetCurrentProcessId},
    },
};

const MARKER: &str = "--shellfox-inspection-v1";
const PACKET: usize = 24;
fn pipe_name(release: &str) -> Result<String> {
    let nonce = release
        .strip_prefix("Local\\ShellfoxPty-")
        .and_then(|name| name.strip_suffix("-GO"))
        .context("invalid inspection nonce")?;
    ensure!(
        nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid inspection nonce"
    );
    Ok(format!("\\\\.\\pipe\\ShellfoxInspect-{nonce}"))
}
pub(crate) struct InspectionGate {
    pipe: OwnedHandle,
    name: String,
    parent: RawProcessIdentity,
    connected: bool,
}
impl InspectionGate {
    pub(crate) fn new(release: &str) -> Result<Self> {
        let name = pipe_name(release)?;
        // Non-inheritable, local-only, first-instance pipe under the token DACL.
        let pipe = unsafe {
            CreateNamedPipeW(
                wide(&name).as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE
                    | PIPE_READMODE_MESSAGE
                    | PIPE_NOWAIT
                    | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                PACKET as u32,
                PACKET as u32,
                0,
                ptr::null(),
            )
        };
        ensure!(
            pipe != INVALID_HANDLE_VALUE && !pipe.is_null(),
            "create private inspection pipe"
        );
        let pipe = unsafe { OwnedHandle::from_raw_handle(pipe.cast()) };
        let parent = RawProcessIdentity {
            pid: unsafe { GetCurrentProcessId() },
            birth: inspection::birth(unsafe { GetCurrentProcess() })?,
        };
        Ok(Self {
            pipe,
            name,
            parent,
            connected: false,
        })
    }
    pub(crate) fn arguments(&self) -> Vec<OsString> {
        vec![
            MARKER.into(),
            self.name.clone().into(),
            self.parent.pid.to_string().into(),
            self.parent.birth.to_string().into(),
        ]
    }
    pub(crate) fn try_capture(
        &mut self,
        job: &Job,
        supervisor: &(dyn portable_pty::Child + Send + Sync),
    ) -> Result<Option<InspectionCapability>> {
        let pipe = self.pipe.as_raw_handle().cast();
        if !self.connected {
            if unsafe { ConnectNamedPipe(pipe, ptr::null_mut()) } == 0 {
                match unsafe { GetLastError() } {
                    ERROR_PIPE_LISTENING => return Ok(None),
                    ERROR_PIPE_CONNECTED => {}
                    _ => anyhow::bail!("connect private inspection pipe"),
                }
            }
            self.connected = true;
        }
        let supervisor_handle = supervisor
            .as_raw_handle()
            .context("missing original supervisor handle")?
            .cast();
        let expected = inspection::process_identity(supervisor_handle)?;
        let mut client_pid = 0;
        ensure!(
            unsafe { GetNamedPipeClientProcessId(pipe, &mut client_pid) } != 0,
            "authenticate inspection client"
        );
        let client = inspection::open_query(client_pid)?;
        ensure!(
            inspection::verify_member(job.handle(), client.as_raw_handle().cast())? == expected,
            "inspection client differs from original supervisor"
        );
        let mut available = 0;
        ensure!(
            unsafe {
                PeekNamedPipe(
                    pipe,
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    &mut available,
                    ptr::null_mut(),
                )
            } != 0,
            "inspect private proof packet"
        );
        if available == 0 {
            return Ok(None);
        }
        ensure!(
            available == PACKET as u32,
            "invalid inspection proof length"
        );
        let mut packet = [0u8; PACKET];
        let mut read = 0;
        ensure!(
            unsafe {
                ReadFile(
                    pipe,
                    packet.as_mut_ptr(),
                    PACKET as u32,
                    &mut read,
                    ptr::null_mut(),
                )
            } != 0
                && read == PACKET as u32,
            "read private inspection proof"
        );
        ensure!(
            &packet[..8] == b"SFIP\x01\0\0\0" && packet[12..16] == [0; 4],
            "invalid inspection proof version"
        );
        let child = RawProcessIdentity {
            pid: u32::from_le_bytes(packet[8..12].try_into().expect("fixed packet field")),
            birth: u64::from_le_bytes(packet[16..24].try_into().expect("fixed packet field")),
        };
        let capability =
            InspectionCapability::from_authenticated_child(job.handle(), expected, child)?;
        let ack = [0xa5u8];
        let mut written = 0;
        ensure!(
            unsafe { WriteFile(pipe, ack.as_ptr(), 1, &mut written, ptr::null_mut()) } != 0
                && written == 1,
            "acknowledge held child proof"
        );
        Ok(Some(capability))
    }
}
pub(crate) struct SupervisorProof {
    name: OsString,
    parent: RawProcessIdentity,
}
impl SupervisorProof {
    pub(crate) fn parent_identity(&self) -> RawProcessIdentity {
        self.parent
    }
    pub(crate) fn parse(args: &mut impl Iterator<Item = OsString>, release: &str) -> Result<Self> {
        ensure!(
            args.next().as_deref() == Some(OsStr::new(MARKER)),
            "missing inspection protocol"
        );
        let name = args.next().context("missing inspection pipe")?;
        ensure!(
            name == OsStr::new(&pipe_name(release)?),
            "inspection pipe nonce mismatch"
        );
        let pid = args
            .next()
            .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
            .context("invalid inspection parent")?;
        let birth = args
            .next()
            .and_then(|value| value.to_str().and_then(|value| value.parse().ok()))
            .context("invalid inspection parent birth")?;
        ensure!(pid != 0 && birth != 0, "missing inspection parent identity");
        Ok(Self {
            name,
            parent: RawProcessIdentity { pid, birth },
        })
    }
    pub(crate) fn publish(&self, child: &std::process::Child) -> Result<()> {
        let pipe = unsafe {
            CreateFileW(
                wide(&self.name).as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_READ
                    | windows_sys::Win32::Foundation::GENERIC_WRITE,
                0,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        ensure!(
            pipe != INVALID_HANDLE_VALUE && !pipe.is_null(),
            "open private inspection pipe"
        );
        let pipe = unsafe { OwnedHandle::from_raw_handle(pipe.cast()) };
        let mut server_pid = 0;
        ensure!(
            unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut server_pid) }
                != 0
                && server_pid == self.parent.pid,
            "inspection server differs from parent"
        );
        let parent = inspection::open_query(server_pid)?;
        ensure!(
            inspection::process_identity(parent.as_raw_handle().cast())? == self.parent,
            "inspection server birth mismatch"
        );
        let identity = inspection::process_identity(child.as_raw_handle().cast())?;
        let mut packet = [0u8; PACKET];
        packet[..8].copy_from_slice(b"SFIP\x01\0\0\0");
        packet[8..12].copy_from_slice(&identity.pid.to_le_bytes());
        packet[16..24].copy_from_slice(&identity.birth.to_le_bytes());
        let mut written = 0;
        ensure!(
            unsafe {
                WriteFile(
                    pipe.as_raw_handle().cast(),
                    packet.as_ptr(),
                    PACKET as u32,
                    &mut written,
                    ptr::null_mut(),
                )
            } != 0
                && written == PACKET as u32,
            "publish exact child identity"
        );
        let mut ack = [0u8];
        let mut read = 0;
        ensure!(
            unsafe {
                ReadFile(
                    pipe.as_raw_handle().cast(),
                    ack.as_mut_ptr(),
                    1,
                    &mut read,
                    ptr::null_mut(),
                )
            } != 0
                && read == 1
                && ack == [0xa5],
            "parent did not capture child proof"
        );
        Ok(())
    }
}
