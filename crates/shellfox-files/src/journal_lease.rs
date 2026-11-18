use crate::Result;
use cap_std::fs::File;
/// Exclusive V2 metadata lease. Existing files are verified, never permission-repaired.
pub(crate) fn acquire_v2(root:&crate::service::journal_anchor::JournalRoot)->Result<File>{
    use crate::{PrivateKind,PrivateAccess,PrivateFileUse};
    let directory=root.directory()?.into_std_file();
    let name=std::ffi::OsStr::new(".file-transfer-stages-v2.lock");
    let file=match crate::open_existing_private_at(&directory,name,PrivateKind::File,PrivateAccess::ExclusiveLease){
        Ok(lease)=>lease.into_file(),
        Err(error)if error.code==crate::ErrorCode::NotFound=>match crate::create_private_file_at(&directory,name,PrivateFileUse::ExclusiveLease){
            Ok(lease)=>lease.into_file(),Err(failure)=>{
                let(error,mut custody)=failure.into_parts();
                if let Some(held)=custody.as_mut(){held.preserve_stale();}
                // Preserve the zero-byte artifact. No user stage/data was admitted and no absence is asserted.
                return Err(error);
            }
        },
        Err(error)=>return Err(error),
    };
    Ok(File::from_std(file))
}
