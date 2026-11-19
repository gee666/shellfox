//! Local file operations with explicit desktop or selected-directory authority.
//! No network transport, upload capability or automatic host authorization.
mod assets;
mod content;
mod contracts;
mod directory_ops;
mod directory_walk;
mod directory_names;
mod directory_plan;
pub use directory_plan::{DirectoryPlan,DirectoryRelation};
pub use directory_plan::create::{DirectoryPreparationFailure,DirectoryPreparationOwner,DirectoryRetention};
mod browser_wsl;
pub use browser_wsl::WslBrowserScope;
mod editor;
mod native_commit_fence;
pub use native_commit_fence::NativeCommitFence;
mod error;
mod image_save;
mod journal;
mod journal_lease;
mod journal_storage;
mod journal_private;
#[cfg(all(test,windows))]
#[path="journal_anchor_test_windows.rs"]
mod private_test_windows;
#[cfg(all(test,windows))]
#[path="../tests/cases/windows_junction.rs"]
mod private_test_junction;
mod journal_publish;
mod mutations;
mod paths;
mod permissions;
mod platform;
#[cfg(unix)]
mod platform_directory_sync;
#[path = "private_identity.rs"]
pub mod handle_identity;
#[cfg(windows)]
mod private_security;
mod private_creation;
mod private_creation_failure;
pub use private_creation_failure::{PrivateCreationFailure,CreationCustody,PrivateCreationEvidence,PrivatePreservationState};
mod private_native;
mod private_verify;
#[cfg(windows)]
mod private_parent_windows;
#[cfg(windows)]
mod private_windows;
#[cfg(windows)]
pub use private_creation::{create_app_directory_at, open_app_directory_at};
pub use private_creation::{create_private_directory_at, create_private_file_at, repair_app_directory_at,
    open_existing_private_at, CreatedPrivate, VerifiedPrivate, PrivateKind, PrivateFileUse, PrivateAccess,PrivateCleanupState,PreparedPrivateDirectoryRef,
    bounded_private_read_at,PrivateRead,PrivateVersion,PRIVATE_METADATA_MAX_BYTES,
    prepare_private_publication_at,OwnedPrivatePublication,PrivatePublicationPhase,PrivatePublishOutcome,
    PrivateDurability,PrivateCleanupDisposition,PublishedPrivate,PrivatePreparationFailure};
pub use handle_identity::HandleIdentity;
pub type PrivateIdentity = HandleIdentity;
mod read_result;
mod service;
mod staged_write;
mod user_stage_custody;
#[cfg(windows)]
mod user_stage_windows;
mod stream_io;
mod transfer_begin;
mod transfer_chunks;
mod transfer_contracts;
mod transfer_registry;
mod upload_contracts;
mod upload_inbox;
mod upload_private;
mod upload_storage;
mod upload_transfer;

pub use assets::{AssetRequest, FileAsset, ImageWrite, INLINE_IMAGE_WRITE_BYTES};
pub use contracts::*;
pub use directory_ops::{
    DirectoryCompletions, DirectoryResolved, FileContext, DIRECTORY_COMPLETION_BYTES,
    DIRECTORY_COMPLETION_LIMIT,
};
pub use editor::{EditorBackend, EditorCancellation, EditorFileSink, EditorFileSource, EditorSession};
#[cfg(target_os = "linux")]
pub use editor::serve_wsl_files;
pub mod wsl_files_ipc;
pub use error::{ErrorCode, FileError, Result};
pub use read_result::ReadResult;
pub use service::FileService;
pub use service::native_directory_adapter::NativeChildIssuer;
pub use service::cap_private_metadata::{ScopedPrivatePublication,ScopedPrivatePreparationFailure,ScopedPublishedReceipt};
pub use transfer_contracts::*;
pub use transfer_registry::{TransferRegistry, TransferUsage};
pub use upload_contracts::*;
pub use upload_inbox::{UploadInbox, UploadUsage};
