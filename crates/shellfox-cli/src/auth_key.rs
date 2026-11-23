//! `network login --auth-key-file`: the same file checks as 0.9 (src/cli/secrets.ts).
//! The key must be a regular, unlinked file only this user (plus SYSTEM and
//! Administrators on Windows) can read.
use crate::output::CliError;
use anyhow::Result;
use shellfox_service::remote::AuthKey;
use std::{io::Read, path::Path};

const LIMIT: usize = 4096;

pub fn read_file(path: &Path) -> Result<AuthKey> {
    let unsafe_file = || CliError::usage("Auth key must be a protected regular file, not a link.");
    let before = std::fs::symlink_metadata(path).map_err(|_| unsafe_file())?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(unsafe_file());
    }
    let mut file = open(path).map_err(|_| unsafe_file())?;
    let after = file.metadata().map_err(|_| unsafe_file())?;
    if !after.is_file() || after.len() > LIMIT as u64 {
        return Err(CliError::usage("Unsafe auth key file."));
    }
    private(&file, &before, &after)?;
    let mut bytes = Vec::new();
    (&mut file).take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(CliError::usage("Auth key exceeds the input limit."));
    }
    validate(&bytes)
}

/// Trims surrounding whitespace; rejects empty keys, inner whitespace and controls.
pub fn validate(bytes: &[u8]) -> Result<AuthKey> {
    let invalid = || CliError::usage("Invalid auth key input.");
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?.trim();
    if text.is_empty()
        || text.len() > LIMIT
        || text.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid());
    }
    AuthKey::new(text.to_owned()).map_err(|_| invalid())
}

#[cfg(unix)]
fn open(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        // A FIFO or device swapped in after the check must not block or become our terminal.
        .custom_flags(
            (rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::NONBLOCK
                | rustix::fs::OFlags::NOCTTY)
                .bits() as i32,
        )
        .open(path)
}
#[cfg(windows)]
fn open(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_FLAG_OPEN_REPARSE_POINT: never follow a link swapped in after the check.
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x0020_0000)
        .open(path)
}

#[cfg(unix)]
fn private(_: &std::fs::File, before: &std::fs::Metadata, after: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let uid = rustix::process::getuid().as_raw();
    if before.dev() != after.dev() || before.ino() != after.ino() || after.nlink() != 1 {
        return Err(CliError::usage("Unsafe auth key file."));
    }
    if after.uid() != uid || after.mode() & 0o077 != 0 {
        return Err(CliError::usage(
            "Auth key file must be owned by the current user with mode 0600 or stricter.",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn private(file: &std::fs::File, _: &std::fs::Metadata, _: &std::fs::Metadata) -> Result<()> {
    // The handle was opened without following links, so its identity is the file checked.
    if !windows::single_link(file).unwrap_or(false) {
        return Err(CliError::usage("Unsafe auth key file."));
    }
    if windows::private(file).unwrap_or(false) {
        Ok(())
    } else {
        Err(CliError::usage("Auth key file must be owned by the current user and readable only by that user, SYSTEM, or Administrators."))
    }
}

#[cfg(windows)]
mod windows {
    use std::{os::windows::io::AsRawHandle, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            EqualSid, GetAce, GetTokenInformation, IsWellKnownSid, TokenUser,
            WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER,
            DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER,
        },
        Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    /// Not a hard link to a file someone else can name.
    pub fn single_link(file: &std::fs::File) -> std::io::Result<bool> {
        // SAFETY: the handle is open and the output struct is plain data.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(info.nNumberOfLinks == 1)
    }

    /// Owner is this user, and every allow ACE names this user, SYSTEM or Administrators.
    pub fn private(file: &std::fs::File) -> std::io::Result<bool> {
        let mut token = null_mut();
        // SAFETY: the pseudo handle is always valid; output points to live storage.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut user = vec![0usize; 64];
        let mut size = 0;
        // SAFETY: the buffer covers the stated size; the SID stays inside it.
        let ok = unsafe {
            let ok = GetTokenInformation(
                token,
                TokenUser,
                user.as_mut_ptr().cast(),
                (user.len() * std::mem::size_of::<usize>()) as u32,
                &mut size,
            );
            CloseHandle(token);
            ok
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: GetTokenInformation filled a TOKEN_USER at the start of the buffer.
        let current = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let (mut owner, mut acl, mut descriptor) = (null_mut(), null_mut(), null_mut());
        // SAFETY: all outputs are valid; the descriptor owns owner and ACL until LocalFree.
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        };
        if code != 0 {
            return Err(std::io::Error::from_raw_os_error(code as i32));
        }
        // SAFETY: owner, ACL and ACEs live in descriptor, freed after this block.
        let allowed = unsafe {
            let trusted = |sid| {
                EqualSid(sid, current) != 0
                    || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                    || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
            };
            // A null DACL grants everyone access.
            !owner.is_null()
                && !acl.is_null()
                && EqualSid(owner, current) != 0
                && (0..u32::from((*acl).AceCount)).all(|index| {
                    let mut ace = null_mut();
                    if GetAce(acl, index, &mut ace) == 0 {
                        return false;
                    }
                    let header = &*ace.cast::<ACE_HEADER>();
                    // Fail closed: a plain deny (1) only narrows access, a plain allow (0)
                    // must name a trusted SID, and callback or object ACEs are refused.
                    match header.AceType {
                        0 => {
                            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                            trusted((&ace.SidStart as *const u32).cast_mut().cast())
                        }
                        1 => true,
                        _ => false,
                    }
                })
        };
        // SAFETY: descriptor came from GetSecurityInfo.
        unsafe { LocalFree(descriptor) };
        Ok(allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_are_trimmed_and_controls_rejected_without_echoing_the_key() {
        assert!(validate(b"  tskey-auth-abc\n").is_ok());
        for bad in [&b""[..], b"two words", b"a\x1bb", &[0xff]] {
            let error = validate(bad).unwrap_err();
            assert!(!format!("{error:#}").contains("two"));
        }
    }
    #[test]
    fn a_private_file_is_read_and_links_or_shared_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("key");
        std::fs::write(&key, "tskey-auth-PRIVATE\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(read_file(&key).is_ok());
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(read_file(&key).is_err());
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&key, &link).unwrap();
            assert!(read_file(&link).is_err());
        }
        // Temp files inherit the user's ACL, so Windows checks the outcome both ways.
        #[cfg(windows)]
        {
            let checked = read_file(&key);
            let private = windows::private(&std::fs::File::open(&key).unwrap()).unwrap();
            assert_eq!(checked.is_ok(), private);
        }
        assert!(read_file(dir.path()).is_err());
    }
}
