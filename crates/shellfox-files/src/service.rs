use crate::{
    content,
    paths::{self, Target},
    *,
};
mod browser_paths;
mod native;
mod resolution;
use cap_std::fs::Dir;
pub use native::NativeCommitFence;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

/// Held from the revision or version check through the rename that commits it, so two
/// writers in this process (any service instance) cannot both pass the same check.
/// Other processes are not covered; see docs/files-api.md.
pub(crate) static MUTATIONS: Mutex<()> = Mutex::new(());

struct Root {
    dir: Dir,
    resolution: RootResolution,
}
enum RootResolution {
    /// An approved directory path; requests may be absolute paths under it.
    PathBound { path: PathBuf },
    /// A held private directory; requests are relative names only.
    HandleOnly,
}

pub struct FileService {
    root: Option<Root>,
    kind: CapabilityKind,
    access: Access,
    revoked: Arc<AtomicBool>,
    inherited_revocations: Vec<Arc<AtomicBool>>,
    native_commit_fences: Vec<Arc<dyn NativeCommitFence>>,
}

impl FileService {
    pub fn approved_root(root: impl AsRef<Path>, access: Access) -> Result<Self> {
        let path = root.as_ref().to_path_buf();
        let dir = paths::open_absolute(&path)?;
        Ok(Self {
            root: Some(Root {
                dir,
                resolution: RootResolution::PathBound { path },
            }),
            kind: CapabilityKind::ApprovedRoot,
            access,
            revoked: Arc::new(AtomicBool::new(false)),
            inherited_revocations: Vec::new(),
            native_commit_fences: Vec::new(),
        })
    }

    /// Trusted local application only. This grants account-path access, not a sandbox.
    /// Do not construct this capability from network or mobile requests.
    pub fn local_desktop(access: Access) -> Self {
        Self {
            root: None,
            kind: CapabilityKind::LocalDesktop,
            access,
            revoked: Arc::new(AtomicBool::new(false)),
            inherited_revocations: Vec::new(),
            native_commit_fences: Vec::new(),
        }
    }

    /// Validate an approved legacy record before migration without requiring its
    /// old directory to exist or touching the filesystem. This grants no access.
    pub fn validate_legacy_root_syntax(path: &Path) -> Result<()> {
        paths::validate_absolute_syntax(path)
    }
    /// Trusted server approval only, never a client flag or empty roots list.
    /// Uses the same current-account path and OS ACL implementation as local UI.
    /// It does not enumerate roots, change tokens, elevate, or bypass revocation.
    pub fn approved_full_account() -> Self {
        Self {
            root: None,
            kind: CapabilityKind::ApprovedFullAccount,
            access: Access::ReadWrite,
            revoked: Arc::new(AtomicBool::new(false)),
            inherited_revocations: Vec::new(),
            native_commit_fences: Vec::new(),
        }
    }
    pub fn capability_kind(&self) -> CapabilityKind {
        self.kind
    }

    /// Waits for an already committing mutation. No later operation may commit.
    pub fn revoke(&self) -> Result<()> {
        let _guard = Self::mutation_lock()?;
        self.revoked.store(true, Ordering::Release);
        Ok(())
    }

    pub(crate) fn authorized(&self, write: bool) -> Result<()> {
        if self.revoked.load(Ordering::Acquire)
            || self
                .inherited_revocations
                .iter()
                .any(|fence| fence.load(Ordering::Acquire))
            || (write && self.access != Access::ReadWrite)
        {
            return Err(FileError::denied());
        }
        if write {
            for fence in &self.native_commit_fences {
                fence.check()?;
            }
        }
        Ok(())
    }

    pub(crate) fn can_derive_root(&self) -> Result<()> {
        self.authorized(true)?;
        if self.inherited_revocations.len() >= 32 {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Directory authority ancestry exceeds its bound.",
            ));
        }
        Ok(())
    }
    /// A narrower root that keeps this one's fences and revocation chain. Revoking the child
    /// does not revoke the parent; revoking the parent fences the child.
    fn derive_bound_root(&self, root: Root) -> Result<Self> {
        self.can_derive_root()?;
        let mut inherited = self.inherited_revocations.clone();
        inherited.push(self.revoked.clone());
        Ok(Self {
            root: Some(root),
            kind: CapabilityKind::ApprovedRoot,
            access: self.access,
            revoked: Arc::new(AtomicBool::new(false)),
            inherited_revocations: inherited,
            native_commit_fences: self.native_commit_fences.clone(),
        })
    }
    pub(crate) fn mutation_lock() -> Result<MutexGuard<'static, ()>> {
        MUTATIONS
            .lock()
            .map_err(|_| FileError::new(ErrorCode::StorageFailed, "File service lock failed."))
    }

    pub(crate) fn target(&self, path: &str) -> Result<Target> {
        let (base, relative, absolute) = self.base(path)?;
        let parent = relative.parent().unwrap_or(Path::new(""));
        let name = relative.file_name().ok_or_else(FileError::denied)?;
        Ok(Target {
            parent: paths::descend(base, parent)?,
            name: PathBuf::from(name),
            absolute,
        })
    }

    fn directory(&self, path: &str) -> Result<(Dir, PathBuf)> {
        let (base, relative, absolute) = self.base(path)?;
        Ok((paths::descend(base, &relative)?, absolute))
    }

    pub fn list(&self, input: &Location) -> Result<FileList> {
        let (dir, path) = self.directory(&input.path)?;
        let mut entries = Vec::new();
        let mut bytes = paths::display(&path)?.len() * 12 + 256;
        let mut truncated = false;
        for entry in dir.entries()? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if paths::validate(&name).is_err() {
                continue;
            }
            let full_path = paths::display(&path.join(&name))?;
            // Six bytes per UTF-8 byte bounds JSON escaping without materializing JSON.
            let cost = 6 * (full_path.len() + name.len()) + 128;
            if entries.len() >= LIST_LIMIT || bytes + cost > LIST_BYTES - 64 {
                truncated = true;
                break;
            }
            let kind = kind(&entry.metadata()?);
            entries.push(FileEntry {
                name,
                path: full_path,
                kind,
            });
            bytes += cost;
        }
        entries.sort_by(|a, b| {
            (b.kind == EntryKind::Directory)
                .cmp(&(a.kind == EntryKind::Directory))
                .then_with(|| a.name.cmp(&b.name))
        });
        let parent_path = if self.root_path().is_some_and(|root| root == path)
            || self.is_private_scope() && path == Path::new(".")
        {
            None
        } else if self.is_private_scope() {
            path.parent()
                .map(|parent| {
                    if parent.as_os_str().is_empty() {
                        Ok(".".to_owned())
                    } else {
                        paths::display(parent)
                    }
                })
                .transpose()?
        } else {
            path.parent().map(paths::display).transpose()?
        };
        self.authorized(false)?;
        Ok(FileList {
            path: paths::display(&path)?,
            parent_path,
            entries,
            truncated,
        })
    }

    pub fn stat(&self, input: &Location) -> Result<FileStat> {
        let (base, relative, absolute) = self.base(&input.path)?;
        let metadata = if relative.as_os_str().is_empty() || relative == Path::new(".") {
            base.dir_metadata()?
        } else {
            let target = self.target(&input.path)?;
            target.parent.symlink_metadata(target.name)?
        };
        self.authorized(false)?;
        Ok(FileStat {
            path: paths::display(&absolute)?,
            kind: kind(&metadata),
            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
        })
    }

    pub fn read_text(&self, input: &Location) -> Result<TextFile> {
        let target = self.target(&input.path)?;
        let (bytes, _) = content::read(
            &mut content::open_regular(&target.parent, &target.name)?,
            TEXT_BYTES,
        )?;
        self.authorized(false)?;
        Ok(TextFile {
            path: paths::display(&target.absolute)?,
            text: content::text(&bytes)?,
            size: bytes.len() as u64,
            revision: content::revision(&bytes),
        })
    }

    pub fn preview(&self, input: &Location) -> Result<Preview> {
        let target = self.target(&input.path)?;
        let (bytes, _) = content::read(
            &mut content::open_regular(&target.parent, &target.name)?,
            PREVIEW_BYTES,
        )?;
        self.authorized(false)?;
        let path = paths::display(&target.absolute)?;
        let revision = content::revision(&bytes);
        if let Some(mime_type) = content::image_mime(&bytes) {
            return Ok(Preview::Image {
                path,
                mime_type: mime_type.to_owned(),
                bytes,
                revision,
            });
        }
        if let Ok(text) = content::text(&bytes) {
            return Ok(Preview::Text(TextFile {
                path,
                text,
                size: bytes.len() as u64,
                revision,
            }));
        }
        Ok(Preview::Binary {
            path,
            size: bytes.len() as u64,
            revision,
        })
    }
}

pub(crate) fn kind(metadata: &cap_std::fs::Metadata) -> EntryKind {
    if paths::reject_link(metadata).is_err() {
        EntryKind::Symlink
    } else if metadata.is_dir() {
        EntryKind::Directory
    } else if metadata.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}
