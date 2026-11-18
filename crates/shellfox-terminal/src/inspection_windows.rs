use super::*;
use std::{
    mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{
        DuplicateHandle, GetLastError, ERROR_MORE_DATA, FILETIME, HANDLE, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    System::{
        JobObjects::{IsProcessInJob, JobObjectBasicProcessIdList, QueryInformationJobObject},
        Threading::{
            GetCurrentProcess, GetProcessId, GetProcessTimes, OpenProcess, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION, SYNCHRONIZATION_SYNCHRONIZE,
        },
    },
};
#[path = "inspection_text.rs"]
mod text;

pub(crate) fn birth(handle: HANDLE) -> Result<u64, InspectionFailure> {
    let mut created: FILETIME = unsafe { mem::zeroed() };
    let mut exit = created;
    let mut kernel = created;
    let mut user = created;
    // SAFETY: the caller retains this process handle; each output is initialized.
    if unsafe { GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) } == 0 {
        return Err(InspectionFailure::Unavailable);
    }
    Ok(u64::from(created.dwLowDateTime) | (u64::from(created.dwHighDateTime) << 32))
}
pub(crate) fn process_identity(handle: HANDLE) -> Result<RawProcessIdentity, InspectionFailure> {
    let pid = unsafe { GetProcessId(handle) };
    if pid == 0 {
        return Err(InspectionFailure::Unavailable);
    }
    Ok(RawProcessIdentity {
        pid,
        birth: birth(handle)?,
    })
}
fn duplicate_query(handle: HANDLE, access: u32) -> Result<OwnedHandle, InspectionFailure> {
    let mut duplicate = ptr::null_mut();
    let current = unsafe { GetCurrentProcess() };
    if unsafe { DuplicateHandle(current, handle, current, &mut duplicate, access, 0, 0) } == 0
        || duplicate.is_null()
    {
        return Err(InspectionFailure::Unavailable);
    }
    // SAFETY: DuplicateHandle returned a new non-inheritable handle owned here.
    Ok(unsafe { OwnedHandle::from_raw_handle(duplicate.cast()) })
}
pub(crate) fn open_query(pid: u32) -> Result<OwnedHandle, InspectionFailure> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZATION_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return Err(InspectionFailure::Unavailable);
    }
    // SAFETY: OpenProcess returned a fresh query-only handle, never a kill handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.cast()) })
}
pub(crate) fn verify_member(
    job: HANDLE,
    process: HANDLE,
) -> Result<RawProcessIdentity, InspectionFailure> {
    let mut member = 0;
    if unsafe { IsProcessInJob(process, job, &mut member) } == 0 || member == 0 {
        return Err(InspectionFailure::OwnershipMismatch);
    }
    process_identity(process)
}
pub(super) struct OwnedInspection {
    job: OwnedHandle,
    root: OwnedHandle,
    root_identity: RawProcessIdentity,
    supervisor: RawProcessIdentity,
}
impl OwnedInspection {
    pub(super) fn launch_evidence(
        &self,
        binding: crate::NativeLaunchBinding,
    ) -> crate::NativeLaunchEvidence {
        crate::launch_privilege::capture_pinned_pty(
            binding,
            self.root.as_raw_handle().cast(),
            self.job.as_raw_handle().cast(),
        )
    }
    pub(super) fn new(
        job: HANDLE,
        supervisor: RawProcessIdentity,
        child: RawProcessIdentity,
    ) -> Result<Self, InspectionFailure> {
        // JOB_OBJECT_QUERY is the documented read-only Job access right.
        let job = duplicate_query(job, 0x0004)?;
        let root = open_query(child.pid)?;
        if verify_member(job.as_raw_handle().cast(), root.as_raw_handle().cast())? != child {
            return Err(InspectionFailure::OwnershipMismatch);
        }
        Ok(Self {
            job,
            root,
            root_identity: child,
            supervisor,
        })
    }
    pub(super) fn capture(&self) -> Result<RawOwnedEvidence, InspectionFailure> {
        let job = self.job.as_raw_handle().cast();
        let root = self.root.as_raw_handle().cast();
        if verify_member(job, root)? != self.root_identity {
            return Err(InspectionFailure::OwnershipMismatch);
        }
        let root_alive = match unsafe { WaitForSingleObject(root, 0) } {
            WAIT_TIMEOUT => true,
            WAIT_OBJECT_0 => false,
            _ => return Err(InspectionFailure::Unavailable),
        };
        let (pids, complete) = if root_alive {
            job_pids(job)?
        } else {
            (Vec::new(), false)
        };
        let capacity = (pids.len() + 1).min(MAX_INSPECTION_PROCESSES);
        let mut snapshot = RawOwnedEvidence {
            platform: InspectionPlatform::Windows,
            root: self.root_identity,
            root_alive,
            complete,
            processes: Vec::with_capacity(capacity),
        };
        let mut budget = capacity * mem::size_of::<RawProcessEvidence>();
        match row(root, self.root_identity, true, &mut budget) {
            Ok(root) => snapshot.processes.push(root),
            Err(InspectionFailure::Bounds | InspectionFailure::Malformed) => {
                snapshot.complete = false;
                snapshot
                    .processes
                    .push(inaccessible(self.root_identity, true));
            }
            Err(error) => return Err(error),
        }
        for pid in pids {
            if pid == self.root_identity.pid {
                continue;
            }
            let process = match open_query(pid) {
                Ok(handle) => handle,
                Err(_) => {
                    snapshot.complete = false;
                    continue;
                }
            };
            // Scope comes from this retained Job. Prove membership and native
            // birth on the held query handle BEFORE any executable/argv read.
            let identity = match verify_member(job, process.as_raw_handle().cast()) {
                Ok(identity) => identity,
                Err(_) => {
                    snapshot.complete = false;
                    continue;
                }
            };
            if identity == self.supervisor {
                continue;
            }
            if snapshot.processes.len() >= MAX_INSPECTION_PROCESSES {
                snapshot.complete = false;
                break;
            }
            match row(process.as_raw_handle().cast(), identity, false, &mut budget) {
                Ok(process) => snapshot.processes.push(process),
                Err(InspectionFailure::Bounds | InspectionFailure::Malformed) => {
                    snapshot.complete = false;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(snapshot)
    }
}
fn job_pids(job: HANDLE) -> Result<(Vec<u32>, bool), InspectionFailure> {
    // A bounded ~160 KiB list on 64-bit Windows. One extra slot is the exact
    // supervisor, excluded from evidence only after handle/birth proof.
    let bytes = 8 + (MAX_INSPECTION_PROCESSES + 1) * mem::size_of::<usize>();
    let mut storage = vec![0usize; bytes.div_ceil(mem::size_of::<usize>())];
    let mut returned = 0;
    let success = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicProcessIdList,
            storage.as_mut_ptr().cast(),
            bytes as u32,
            &mut returned,
        )
    } != 0;
    if !success && unsafe { GetLastError() } != ERROR_MORE_DATA {
        return Err(InspectionFailure::Unavailable);
    }
    decode_pids(&storage, success)
}
fn decode_pids(storage: &[usize], success: bool) -> Result<(Vec<u32>, bool), InspectionFailure> {
    if std::mem::size_of_val(storage) < 8 {
        return Err(InspectionFailure::Malformed);
    }
    let header = storage.as_ptr().cast::<u32>();
    // Success or ERROR_MORE_DATA initializes both counts; only copied slots are read.
    let assigned = unsafe { *header } as usize;
    let count = unsafe { *header.add(1) } as usize;
    if count > MAX_INSPECTION_PROCESSES + 1
        || assigned < count
        || 8 + count * mem::size_of::<usize>() > std::mem::size_of_val(storage)
    {
        return Err(InspectionFailure::Malformed);
    }
    let pids = unsafe { storage.as_ptr().cast::<u8>().add(8).cast::<usize>() };
    let mut result = Vec::with_capacity(count);
    let mut seen = std::collections::HashSet::with_capacity(count);
    for index in 0..count {
        let pid =
            u32::try_from(unsafe { *pids.add(index) }).map_err(|_| InspectionFailure::Malformed)?;
        if pid == 0 || !seen.insert(pid) {
            return Err(InspectionFailure::Malformed);
        }
        result.push(pid);
    }
    Ok((result, success && assigned == count))
}
fn inaccessible(identity: RawProcessIdentity, is_root: bool) -> RawProcessEvidence {
    RawProcessEvidence {
        identity,
        is_root,
        accessible: false,
        executable: None,
        process_name: None,
        argv: None,
    }
}
fn row(
    handle: HANDLE,
    identity: RawProcessIdentity,
    is_root: bool,
    budget: &mut usize,
) -> Result<RawProcessEvidence, InspectionFailure> {
    match text::read(handle) {
        Ok((executable, process_name, argv)) => {
            // Reserved row-vector storage is already included by capture().
            let mut next = budget
                .checked_add((argv.len() + 2) * mem::size_of::<OsString>())
                .ok_or(InspectionFailure::Bounds)?;
            for field in [&executable, &process_name].into_iter().chain(argv.iter()) {
                use std::os::windows::ffi::OsStrExt;
                let bytes = field
                    .to_string_lossy()
                    .len()
                    .max(field.encode_wide().count() * 2);
                if bytes > MAX_INSPECTION_FIELD_BYTES {
                    return Err(InspectionFailure::Bounds);
                }
                next = next.checked_add(bytes).ok_or(InspectionFailure::Bounds)?;
                if next > MAX_INSPECTION_BYTES {
                    return Err(InspectionFailure::Bounds);
                }
            }
            *budget = next;
            Ok(RawProcessEvidence {
                identity,
                is_root,
                accessible: true,
                executable: Some(executable),
                process_name: Some(process_name),
                argv: Some(argv),
            })
        }
        Err(InspectionFailure::Bounds | InspectionFailure::Malformed) => {
            Err(InspectionFailure::Bounds)
        }
        Err(_) => Ok(inaccessible(identity, is_root)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn twenty_thousand_row_bound_reports_partial_not_false_complete() {
        let count = MAX_INSPECTION_PROCESSES + 1;
        let bytes = 8 + count * mem::size_of::<usize>();
        let mut storage = vec![0usize; bytes.div_ceil(mem::size_of::<usize>())];
        let header = storage.as_mut_ptr().cast::<u32>();
        unsafe {
            *header = 30_000;
            *header.add(1) = count as u32;
        }
        let pids = unsafe { storage.as_mut_ptr().cast::<u8>().add(8).cast::<usize>() };
        for index in 0..count {
            unsafe {
                *pids.add(index) = index + 1;
            }
        }
        let (rows, complete) = decode_pids(&storage, false).unwrap();
        assert_eq!(rows.len(), 20_001);
        assert!(!complete);
        unsafe {
            *header.add(1) = 20_002;
        }
        assert!(decode_pids(&storage, false).is_err());
    }
    #[test]
    fn a_numeric_pid_and_birth_outside_the_exact_job_grant_no_read_capability() {
        let job = crate::windows::Job::new().unwrap();
        let current = process_identity(unsafe { GetCurrentProcess() }).unwrap();
        assert!(matches!(
            OwnedInspection::new(job.handle(), current, current),
            Err(InspectionFailure::OwnershipMismatch)
        ));
    }
}
