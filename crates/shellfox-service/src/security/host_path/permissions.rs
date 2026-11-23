//! Unix permission mutations use held descriptors, never re-resolved paths.
use super::*;
use cap_std::fs::MetadataExt as UnixMetadataExt;
use std::{ffi::OsStr, os::unix::fs::PermissionsExt as StdPermissionsExt};
fn current_user() -> u32 {
    rustix::process::getuid().as_raw()
}
fn readable_directory(file: &std::fs::File) -> Result<std::fs::File> {
    use rustix::fs::{fstat, openat, FileType, Mode, OFlags};
    let original = fstat(file)?;
    ensure!(
        FileType::from_raw_mode(original.st_mode) == FileType::Directory,
        "Held resource is not a directory"
    );
    // cap-std may retain O_PATH. fgetxattr/fchmod require a readable directory
    // descriptor. Reopen only '.' from that SAME held directory, never its path.
    let fd = openat(
        file,
        ".",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let actual = fstat(&fd)?;
    ensure!(
        original.st_dev == actual.st_dev
            && original.st_ino == actual.st_ino
            && original.st_uid == actual.st_uid
            && original.st_mode == actual.st_mode,
        "Readable directory differs from original held directory"
    );
    Ok(std::fs::File::from(fd))
}
fn verify_acl(file: &std::fs::File) -> Result<()> {
    let readable;
    let inspected = if file.metadata()?.is_dir() {
        readable = readable_directory(file)?;
        &readable
    } else {
        file
    };
    shellfox_transport::verify_held_acl(inspected)
        .context("Host resource has access or inheritance ACLs")
}

fn verify_ancestor(dir: &Dir) -> Result<()> {
    let file = dir.try_clone()?.into_std_file();
    #[cfg(target_os = "macos")]
    return shellfox_transport::verify_held_traversal_acl(&readable_directory(&file)?)
        .context("Host ancestor ACL grants access or cannot be verified");
    #[cfg(not(target_os = "macos"))]
    verify_acl(&file)
}
fn owned(info: &Metadata, user: u32) -> Result<()> {
    ensure!(info.uid() == user, "Foreign host resource owner");
    Ok(())
}
fn child_directory(parent: &Dir, name: &OsStr, user: u32, verify: bool) -> Result<Dir> {
    let before = parent.symlink_metadata(name)?;
    safe(&before)?;
    owned(&before, user)?;
    let child = parent.open_dir_nofollow(name)?;
    let opened = child.dir_metadata()?;
    safe(&opened)?;
    owned(&opened, user)?;
    ensure!(same(&before, &opened), "Host child changed while opening");
    if verify {
        verify_acl(&child.try_clone()?.into_std_file())?;
    }
    Ok(child)
}
pub(super) fn protect_directory(dir: &Dir, user: u32) -> Result<()> {
    let info = dir.dir_metadata()?;
    safe(&info)?;
    owned(&info, user)?;
    // File::set_permissions acts directly on this duplicated directory fd.
    // Renaming/replacing its path cannot redirect chmod to another directory.
    let file = readable_directory(&dir.try_clone()?.into_std_file())?;
    #[cfg(not(target_os = "macos"))]
    verify_acl(&file)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
    #[cfg(target_os = "macos")]
    shellfox_transport::tighten_held_acl(&file)
        .context("Tighten ACL on the owned host directory")?;
    Ok(())
}
impl HostRoot {
    pub fn validate_creation_ancestors(&self) -> Result<()> {
        let user = current_user();
        let ancestor_count = self.anchors.len() - usize::from(self.exists());
        for anchor in &self.anchors[..ancestor_count] {
            let info = anchor.handle.dir_metadata()?;
            verify_ancestor(&anchor.handle)?;
            let system = info.uid() == 0 || (cfg!(target_os = "android") && info.uid() == 1000);
            ensure!(system || info.uid() == user, "Foreign host ancestor owner");
            // Readable 0755 directories and sticky system /tmp are safe; a
            // non-sticky group/world-writable ancestor can replace our entry.
            ensure!(
                info.mode() & 0o022 == 0 || info.mode() & 0o1000 != 0,
                "Foreign-writable host ancestor"
            );
        }
        if self.exists() {
            let root = &self.anchors.last().context("Missing host root")?.handle;
            owned(&root.dir_metadata()?, user)?;
            #[cfg(not(target_os = "macos"))]
            verify_acl(&root.try_clone()?.into_std_file())?;
        }
        self.revalidate()
    }
    pub fn protect(&self, path: &Path, directory: bool) -> Result<()> {
        self.revalidate()?;
        ensure!(self.exists(), "Cannot protect an absent host root");
        let user = current_user();
        let mut dir = self
            .anchors
            .last()
            .context("Missing host root")?
            .handle
            .try_clone()?;
        owned(&dir.dir_metadata()?, user)?;
        let mut names = Vec::new();
        for component in path.strip_prefix(&self.path)?.components() {
            let Component::Normal(name) = component else {
                anyhow::bail!("Non-original private child path");
            };
            names.push(name);
        }
        if directory {
            let mut names = names.into_iter().peekable();
            while let Some(name) = names.next() {
                // Only the final, owned directory is changed. Ancestors are checked.
                let verify = !cfg!(target_os = "macos") || names.peek().is_some();
                dir = child_directory(&dir, name, user, verify)?;
            }
            protect_directory(&dir, user)?;
        } else {
            let file = self.open_regular(path)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        self.revalidate()
    }
    pub fn open_regular(&self, path: &Path) -> Result<std::fs::File> {
        use rustix::fs::{fstat, openat, statat, AtFlags, FileType, Mode, OFlags};
        self.revalidate()?;
        let user = current_user();
        let mut dir = self
            .anchors
            .last()
            .context("Missing host root")?
            .handle
            .try_clone()?;
        let mut names = Vec::new();
        for component in path.strip_prefix(&self.path)?.components() {
            let Component::Normal(name) = component else {
                anyhow::bail!("Non-original private file path");
            };
            names.push(name);
        }
        let name = names.pop().context("Host root is not a private file")?;
        for name in names {
            dir = child_directory(&dir, name, user, true)?;
        }
        let before = statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW)?;
        ensure!(
            FileType::from_raw_mode(before.st_mode) == FileType::RegularFile
                && before.st_uid == user
                && before.st_nlink == 1,
            "Unsafe private file"
        );
        // NONBLOCK is set BEFORE open, so a substituted FIFO cannot wait here.
        let fd = openat(
            &dir,
            name,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let opened = fstat(&fd)?;
        ensure!(
            FileType::from_raw_mode(opened.st_mode) == FileType::RegularFile
                && opened.st_uid == user
                && opened.st_nlink == 1
                && opened.st_dev == before.st_dev
                && opened.st_ino == before.st_ino,
            "Private file changed while opening"
        );
        let file = std::fs::File::from(fd);
        verify_acl(&file)?;
        self.revalidate()?;
        Ok(file)
    }
}
