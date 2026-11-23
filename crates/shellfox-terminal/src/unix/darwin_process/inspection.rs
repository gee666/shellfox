//! Read-only evidence for `session stats` and activity matching: the tab's own
//! session, run by this user, found the same way cleanup finds it. Paths and
//! argv are read only from those members. Nothing here signals.
use super::{birth, own, process_info, session_pids, Leader, Pid, PROC_FLAG_PSUGID};
use crate::inspection::{
    InspectionFailure as Error, InspectionPlatform, RawOwnedEvidence, RawProcessEvidence,
    RawProcessIdentity, MAX_INSPECTION_BYTES, MAX_INSPECTION_FIELD_BYTES, MAX_INSPECTION_PROCESSES,
};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::sync::OnceLock;

const MAX_ARGS: usize = 4096;
/// KERN_PROCARGS2 returns argv and the environment, up to ARG_MAX (1 MiB).
const MAX_PROCARGS: usize = 4 * 1024 * 1024;

pub(crate) struct Inspection {
    sid: Pid,
    uid: libc::uid_t,
    birth: u64,
}

impl Leader {
    /// The capability is bound to this leader's birth, so a reused PID never matches.
    pub(in crate::unix) fn inspection(&self) -> Option<Inspection> {
        let info = process_info(self.sid, false).filter(|info| own(info, self.uid))?;
        Some(Inspection {
            sid: self.sid,
            uid: self.uid,
            birth: birth(&info),
        })
    }
}

impl Inspection {
    pub(crate) fn capture(&self) -> Result<RawOwnedEvidence, Error> {
        let root = RawProcessIdentity {
            pid: self.sid as u32,
            birth: self.birth,
        };
        let alive = process_info(self.sid, false)
            .is_some_and(|info| birth(&info) == self.birth && own(&info, self.uid))
            && unsafe { libc::getsid(self.sid) } == self.sid;
        if !alive {
            return Ok(RawOwnedEvidence {
                platform: InspectionPlatform::Unix,
                root,
                root_alive: false,
                complete: true,
                processes: Vec::new(),
            });
        }
        let mut complete = true;
        let mut processes = Vec::new();
        let mut buffer = Vec::new();
        let mut total = 0;
        for pid in session_pids(self.sid, self.uid) {
            let Some(info) = process_info(pid, false).filter(|info| own(info, self.uid)) else {
                continue;
            };
            let memory = resident(pid);
            // A process that changed credentials may hold privileged data.
            let readable = info.pbi_flags & PROC_FLAG_PSUGID == 0;
            let executable = readable.then(|| executable(pid)).flatten();
            let argv = readable.then(|| arguments(pid, &mut buffer)).flatten();
            // The same process, session and owner after the reads.
            let same = process_info(pid, false).is_some_and(|again| {
                birth(&again) == birth(&info)
                    && own(&again, self.uid)
                    && again.pbi_flags & PROC_FLAG_PSUGID == info.pbi_flags & PROC_FLAG_PSUGID
            }) && unsafe { libc::getsid(pid) } == self.sid;
            if !same {
                complete = false;
                continue;
            }
            if processes.len() >= MAX_INSPECTION_PROCESSES {
                complete = false;
                break;
            }
            total += executable.as_ref().map_or(0, |path| path.len())
                + argv.iter().flatten().map(|arg| arg.len()).sum::<usize>();
            if total > MAX_INSPECTION_BYTES {
                return Err(Error::Bounds);
            }
            let identity = RawProcessIdentity {
                pid: pid as u32,
                birth: birth(&info),
            };
            processes.push(RawProcessEvidence {
                identity,
                is_root: identity == root,
                accessible: executable.is_some() && argv.is_some(),
                executable,
                process_name: Some(name(&info)),
                argv,
                parent_pid: Some(info.pbi_ppid),
                memory_bytes: memory,
                started_ms: Some(info.pbi_start_tvsec * 1000 + info.pbi_start_tvusec / 1000),
            });
        }
        let root_alive = processes.iter().any(|p| p.is_root);
        if !root_alive {
            return Err(Error::Unavailable);
        }
        Ok(RawOwnedEvidence {
            platform: InspectionPlatform::Unix,
            root,
            root_alive,
            complete,
            processes,
        })
    }
}

fn executable(pid: Pid) -> Option<OsString> {
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: proc_pidpath writes at most `path.len()` bytes.
    let length = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    let length = usize::try_from(length)
        .ok()
        .filter(|length| (1..path.len()).contains(length))?;
    path.truncate(length);
    (path[0] == b'/').then(|| OsString::from_vec(path))
}

/// argv from KERN_PROCARGS2; the environment that follows is never kept.
fn arguments(pid: Pid, buffer: &mut Vec<u8>) -> Option<Vec<OsString>> {
    buffer.resize(arg_max()?, 0);
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut length = buffer.len();
    // SAFETY: the kernel writes at most `length` bytes into `buffer`.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    (result == 0).then(|| parse_arguments(&buffer[..length.min(buffer.len())]))?
}

fn arg_max() -> Option<usize> {
    static ARG_MAX: OnceLock<Option<usize>> = OnceLock::new();
    *ARG_MAX.get_or_init(|| {
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        let mut value: libc::c_int = 0;
        let mut size = size_of::<libc::c_int>();
        // SAFETY: a read-only query into one c_int.
        let result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                std::ptr::addr_of_mut!(value).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        usize::try_from(value)
            .ok()
            .filter(|value| result == 0 && (1..=MAX_PROCARGS).contains(value))
    })
}

/// Layout: argc (native i32), the saved exec path, NUL padding, then argc
/// NUL-terminated strings. An empty argv[0] is indistinguishable from padding.
fn parse_arguments(bytes: &[u8]) -> Option<Vec<OsString>> {
    let (count, rest) = bytes.split_first_chunk::<4>()?;
    let argc = usize::try_from(i32::from_ne_bytes(*count))
        .ok()
        .filter(|argc| (1..=MAX_ARGS).contains(argc))?;
    let rest = &rest[rest.iter().position(|b| *b == 0)?..];
    let mut rest = &rest[rest.iter().position(|b| *b != 0)?..];
    let mut argv = Vec::with_capacity(argc);
    let mut size = 0;
    for _ in 0..argc {
        let end = rest.iter().position(|b| *b == 0)?;
        size += end;
        if size > MAX_INSPECTION_FIELD_BYTES {
            return None;
        }
        argv.push(OsString::from_vec(rest[..end].to_vec()));
        rest = &rest[end + 1..];
    }
    Some(argv)
}

fn resident(pid: Pid) -> Option<u64> {
    let mut task: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: proc_pidinfo fills at most `size` bytes of this owned struct.
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            std::ptr::addr_of_mut!(task).cast(),
            size,
        )
    };
    (bytes == size).then_some(task.pti_resident_size)
}

fn name(info: &libc::proc_bsdinfo) -> OsString {
    let raw = if info.pbi_name[0] != 0 {
        &info.pbi_name[..]
    } else {
        &info.pbi_comm[..]
    };
    let bytes: Vec<u8> = raw
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    OsString::from_vec(bytes)
}

#[cfg(test)]
mod tests {
    use super::parse_arguments;

    fn procargs(argc: i32, body: &[u8]) -> Vec<u8> {
        let mut bytes = argc.to_ne_bytes().to_vec();
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn argv_skips_the_exec_path_padding_and_environment() {
        let bytes = procargs(
            3,
            b"/usr/bin/node\0\0\0\0node\0/opt/claude/cli.js\0\0HOME=/x\0",
        );
        let argv = parse_arguments(&bytes).unwrap();
        assert_eq!(argv, ["node", "/opt/claude/cli.js", ""]);
    }

    #[test]
    fn truncated_or_implausible_argv_is_rejected() {
        assert!(parse_arguments(&procargs(2, b"/bin/sh\0\0sh\0")).is_none());
        assert!(parse_arguments(&procargs(0, b"/bin/sh\0\0sh\0")).is_none());
        assert!(parse_arguments(&procargs(-1, b"/bin/sh\0\0sh\0")).is_none());
        assert!(parse_arguments(b"\x01\0").is_none());
    }

    #[test]
    fn this_process_reads_its_own_path_and_argv() {
        let pid = std::process::id() as libc::pid_t;
        let path = super::executable(pid).unwrap();
        assert_eq!(path, std::env::current_exe().unwrap().into_os_string());
        let argv = super::arguments(pid, &mut Vec::new()).unwrap();
        assert_eq!(argv, std::env::args_os().collect::<Vec<_>>());
    }
}
