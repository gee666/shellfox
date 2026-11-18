//! Finite target-only carrier over a nonce-named, peer-authenticated startup pipe.
use crate::{
    inspection,
    program_env::{
        wire::{self, Packet},
        TrustedProgramEnv,
    },
    windows::{wide, Job},
};
use anyhow::{ensure, Context, Result};
use std::{
    ffi::{OsStr, OsString},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        GetLastError, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, GENERIC_READ,
        INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING,
        PIPE_ACCESS_DUPLEX,
    },
    System::Pipes::*,
};
pub(crate) const MARKER: &str = "--shellfox-program-env-v1";
fn name(release: &str) -> Result<String> {
    let nonce = release
        .strip_prefix("Local\\ShellfoxPty-")
        .and_then(|value| value.strip_suffix("-GO"))
        .context("Invalid program carrier nonce.")?;
    ensure!(
        nonce.len() == 32 && nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid program carrier nonce."
    );
    Ok(format!("\\\\.\\pipe\\ShellfoxProgramEnv-{nonce}"))
}
pub(crate) struct ProgramGate {
    pipe: OwnedHandle,
    name: String,
    packet: Packet,
    connected: bool,
    sent: bool,
}
impl ProgramGate {
    pub(crate) fn new(release: &str, environment: &TrustedProgramEnv) -> Result<Self> {
        let name = name(release)?;
        let payload = environment.packet()?;
        let mut packet = Packet((payload.0.len() as u32).to_le_bytes().to_vec());
        packet.0.extend_from_slice(&payload.0);
        let pipe = unsafe {
            CreateNamedPipeW(
                wide(&name).as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE
                    | PIPE_READMODE_MESSAGE
                    | PIPE_NOWAIT
                    | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                (wire::MAX_PACKET + 4) as u32,
                1,
                0,
                ptr::null(),
            )
        };
        ensure!(
            pipe != INVALID_HANDLE_VALUE && !pipe.is_null(),
            "Create private program carrier failed."
        );
        Ok(Self {
            pipe: unsafe { OwnedHandle::from_raw_handle(pipe.cast()) },
            name,
            packet,
            connected: false,
            sent: false,
        })
    }
    pub(crate) fn arguments(&self) -> [OsString; 2] {
        [MARKER.into(), self.name.clone().into()]
    }
    pub(crate) fn poll(
        &mut self,
        job: &Job,
        supervisor: &(dyn portable_pty::Child + Send + Sync),
    ) -> Result<()> {
        if self.sent {
            return Ok(());
        }
        let pipe = self.pipe.as_raw_handle().cast();
        if !self.connected {
            if unsafe { ConnectNamedPipe(pipe, ptr::null_mut()) } == 0 {
                match unsafe { GetLastError() } {
                    ERROR_PIPE_LISTENING => return Ok(()),
                    ERROR_PIPE_CONNECTED => {}
                    _ => anyhow::bail!("Connect private program carrier failed."),
                }
            }
            self.connected = true;
        }
        let expected = inspection::process_identity(
            supervisor
                .as_raw_handle()
                .context("Original supervisor handle missing.")?
                .cast(),
        )?;
        let mut pid = 0;
        ensure!(
            unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } != 0,
            "Program carrier peer unavailable."
        );
        let actual = inspection::open_query(pid)?;
        ensure!(
            inspection::verify_member(job.handle(), actual.as_raw_handle().cast())? == expected,
            "Program carrier peer mismatch."
        );
        let mut written = 0;
        ensure!(
            unsafe {
                WriteFile(
                    pipe,
                    self.packet.0.as_ptr(),
                    self.packet.0.len() as u32,
                    &mut written,
                    ptr::null_mut(),
                )
            } != 0
                && written as usize == self.packet.0.len(),
            "Send private program carrier failed."
        );
        self.sent = true;
        Ok(())
    }
}
/// Returned parsing never supplies bootstrap/global environment or a program.
pub(crate) fn parse(
    first: OsString,
    args: &mut impl Iterator<Item = OsString>,
    release: &str,
) -> Result<(Option<OsString>, OsString)> {
    if first != OsStr::new(MARKER) {
        return Ok((None, first));
    }
    let pipe = args.next().context("Program carrier pipe missing.")?;
    ensure!(
        pipe == OsStr::new(&name(release)?),
        "Program carrier nonce mismatch."
    );
    Ok((
        Some(pipe),
        args.next().context("Bound target program missing.")?,
    ))
}
pub(crate) fn apply(
    pipe: &OsStr,
    parent: inspection::RawProcessIdentity,
    program: &Path,
    command: &mut std::process::Command,
) -> Result<()> {
    let handle = unsafe {
        CreateFileW(
            wide(pipe).as_ptr(),
            GENERIC_READ,
            0,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    ensure!(
        handle != INVALID_HANDLE_VALUE && !handle.is_null(),
        "Open private program carrier failed."
    );
    let pipe = unsafe { OwnedHandle::from_raw_handle(handle.cast()) };
    let mut pid = 0;
    ensure!(
        unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle().cast(), &mut pid) } != 0
            && pid == parent.pid,
        "Program carrier server mismatch."
    );
    let server = inspection::open_query(pid)?;
    ensure!(
        inspection::process_identity(server.as_raw_handle().cast())? == parent,
        "Program carrier server birth mismatch."
    );
    let mut length = [0u8; 4];
    read_exact(&pipe, &mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(
        length <= wire::MAX_PACKET,
        "Program carrier exceeds its bound."
    );
    let mut packet = Packet(vec![0; length]);
    read_exact(&pipe, &mut packet.0)?;
    wire::decode(&packet.0)?.apply(program, command)
}
fn read_exact(pipe: &OwnedHandle, mut bytes: &mut [u8]) -> Result<()> {
    while !bytes.is_empty() {
        let mut read = 0;
        ensure!(
            unsafe {
                ReadFile(
                    pipe.as_raw_handle().cast(),
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    &mut read,
                    ptr::null_mut(),
                )
            } != 0
                && read > 0,
            "Read private program carrier failed."
        );
        bytes = &mut bytes[read as usize..];
    }
    Ok(())
}
