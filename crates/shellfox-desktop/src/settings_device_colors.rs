//! Device-local swatches. Storage capability is supplied by validated client
//! configuration, never inferred from host data, the executable or run scratch.
use super::api::{ColorKind, CustomColors};
use shellfox_files::{CapabilityKind, EntryKind, FileService, Location};
use shellfox_service::system::{NativeSystemFiles, SystemFiles};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
pub struct DeviceColors {
    path: PathBuf,
    files: Arc<dyn SystemFiles>,
    proof: Arc<FileService>,
    gate: Mutex<()>,
}
impl DeviceColors {
    pub fn native_anchored(directory: &Path, proof: Arc<FileService>) -> anyhow::Result<Self> {
        Self::anchored(directory, proof, Arc::new(NativeSystemFiles))
    }
    fn anchored(
        directory: &Path,
        proof: Arc<FileService>,
        files: Arc<dyn SystemFiles>,
    ) -> anyhow::Result<Self> {
        shellfox_service::system::validate_system_path(directory)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let store = Self {
            path: directory.join("shellfox-device-colors-v1.txt"),
            files,
            proof,
            gate: Mutex::new(()),
        };
        store.verify()?;
        Ok(store)
    }
    fn verify(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.proof.capability_kind() == CapabilityKind::ApprovedRoot
                && self.proof.root_path() == self.path.parent(),
            "Device color proof does not match the configured approved root."
        );
        let root = self.proof.stat(&Location { path: ".".into() })?;
        anyhow::ensure!(
            root.kind == EntryKind::Directory,
            "Device color root is not a directory."
        );
        Ok(())
    }
    pub fn read(&self) -> anyhow::Result<CustomColors> {
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Device color store is unavailable."))?;
        self.verify()?;
        let result = self.files.read(&self.path);
        self.verify()?;
        let file = result.map_err(|error| anyhow::anyhow!(error.message))?;
        let colors = decode(file.as_ref().map(|file| file.bytes.as_slice()))?;
        self.verify()?;
        Ok(colors)
    }
    pub fn save(&self, kind: ColorKind, color: &str) -> anyhow::Result<CustomColors> {
        shellfox_core::validation::color(color)?;
        let color = color.to_ascii_lowercase();
        let _guard = self
            .gate
            .lock()
            .map_err(|_| anyhow::anyhow!("Device color store is unavailable."))?;
        self.verify()?;
        let result = self.files.read(&self.path);
        self.verify()?;
        let old = result.map_err(|error| anyhow::anyhow!(error.message))?;
        let mut current = decode(old.as_ref().map(|file| file.bytes.as_slice()))?;
        let before = current.clone();
        super::colors::add(&mut current, kind, &color);
        if before == current {
            self.verify()?;
            return Ok(current);
        }
        let bytes = encode(&current);
        self.verify()?;
        let result = self
            .files
            .replace(&self.path, old.as_ref(), Some(bytes.as_bytes()), false);
        self.verify()?;
        result.map_err(|error| anyhow::anyhow!(error.message))?;
        Ok(current)
    }
}
fn decode(bytes: Option<&[u8]>) -> anyhow::Result<CustomColors> {
    let Some(bytes) = bytes else {
        return Ok(CustomColors::default());
    };
    anyhow::ensure!(bytes.len() <= 1024, "Invalid device color store size.");
    let text = std::str::from_utf8(bytes)?;
    let mut lines = text.lines();
    anyhow::ensure!(
        lines.next() == Some("Shellfox device colors 1"),
        "Unsupported device color store version."
    );
    let mut result = CustomColors::default();
    let mut kinds = std::collections::HashSet::new();
    for line in lines {
        let (kind, values) = line
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("Invalid device color record."))?;
        anyhow::ensure!(kinds.insert(kind), "Duplicate device color record.");
        let target = match kind {
            "accent" => &mut result.accent,
            "background" => &mut result.background,
            _ => anyhow::bail!("Unknown device color record."),
        };
        let color_kind = if kind == "accent" {
            ColorKind::Accent
        } else {
            ColorKind::Background
        };
        *target = super::colors::normalize(color_kind, values.split(','));
    }
    anyhow::ensure!(kinds.len() == 2, "Incomplete device color store.");
    Ok(result)
}
fn encode(colors: &CustomColors) -> String {
    format!(
        "Shellfox device colors 1\naccent={}\nbackground={}\n",
        colors.accent.join(","),
        colors.background.join(",")
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_files::Access;
    fn native(directory: &Path) -> anyhow::Result<DeviceColors> {
        let proof = Arc::new(FileService::approved_root(directory, Access::ReadWrite)?);
        DeviceColors::native_anchored(directory, proof)
    }
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn scratch() -> Scratch {
        let path = std::env::current_dir().unwrap().join("tmp").join(format!(
            "settings-device-anchor-{}",
            shellfox_core::SessionId::new()
        ));
        std::fs::create_dir(&path).unwrap();
        Scratch(path)
    }
    fn directory_link(alias: &Path, target: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, alias).unwrap();
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let result = std::process::Command::new("cmd.exe")
                .args(["/d", "/c", "mklink", "/J"])
                .arg(alias)
                .arg(target)
                .creation_flags(0x08000000)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "Owned junction setup failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    fn unlink_directory(alias: &Path) {
        #[cfg(unix)]
        std::fs::remove_file(alias).unwrap();
        #[cfg(windows)]
        std::fs::remove_dir(alias).unwrap();
    }
    #[test]
    fn anchored_store_rejects_wrong_root_and_local_desktop_proofs() {
        let fixture = scratch();
        let root = fixture.0.join("bound");
        let other = fixture.0.join("other");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&other).unwrap();
        let proof = Arc::new(FileService::approved_root(&other, Access::ReadWrite).unwrap());
        assert!(DeviceColors::native_anchored(&root, proof).is_err());
        assert!(
            DeviceColors::native_anchored(
                &root,
                Arc::new(FileService::local_desktop(Access::ReadWrite))
            )
            .is_err()
        );
    }
    #[test]
    fn raw_alias_is_not_canonicalized_into_an_approved_store() {
        let fixture = scratch();
        let outside = fixture.0.join("outside");
        let alias = fixture.0.join("alias");
        std::fs::create_dir(&outside).unwrap();
        directory_link(&alias, &outside);
        assert!(native(&alias).is_err());
        unlink_directory(&alias);
    }
    #[test]
    fn lifetime_anchor_blocks_or_rejects_outside_alias_replacement() {
        let fixture = scratch();
        let root = fixture.0.join("bound");
        let held = fixture.0.join("held");
        let outside = fixture.0.join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let store = native(&root).unwrap();
        store.save(ColorKind::Accent, "#123456").unwrap();
        let external = outside.join("shellfox-device-colors-v1.txt");
        let sentinel = b"Shellfox device colors 1\naccent=#abcdef\nbackground=\n";
        std::fs::write(&external, sentinel).unwrap();
        if let Err(error) = std::fs::rename(&root, &held) {
            #[cfg(windows)]
            {
                // Windows' live approved directory handle itself denies this
                // replacement. Assert the denial, not a skipped attack test.
                assert_eq!(error.raw_os_error(), Some(32));
                assert_eq!(store.read().unwrap().accent, ["#123456"]);
                store.save(ColorKind::Accent, "#234567").unwrap();
                assert_eq!(std::fs::read(&external).unwrap(), sentinel);
                return;
            }
            #[cfg(not(windows))]
            panic!("Owned directory rename failed: {error}");
        }
        directory_link(&root, &outside);
        assert!(store.read().is_err());
        assert!(store.save(ColorKind::Accent, "#234567").is_err());
        assert_eq!(std::fs::read(&external).unwrap(), sentinel);
        unlink_directory(&root);
    }
    #[test]
    fn retained_proof_revocation_blocks_later_storage_operations() {
        let fixture = scratch();
        let proof = Arc::new(FileService::approved_root(&fixture.0, Access::ReadWrite).unwrap());
        let store = DeviceColors::native_anchored(&fixture.0, proof.clone()).unwrap();
        store.save(ColorKind::Accent, "#123456").unwrap();
        proof.revoke().unwrap();
        assert!(store.read().is_err());
        assert!(store.save(ColorKind::Accent, "#234567").is_err());
    }
    #[test]
    fn owned_device_store_reopens_and_keeps_last_three_without_cross_kind_loss() {
        let path = std::env::current_dir().unwrap().join("tmp").join(format!(
            "settings-device-colors-{}",
            shellfox_core::SessionId::new()
        ));
        std::fs::create_dir(&path).unwrap();
        let scratch = Scratch(path);
        let store = native(&scratch.0).unwrap();
        assert!(store.read().unwrap().accent.is_empty());
        store.save(ColorKind::Background, "#121212").unwrap();
        for color in ["#123456", "#234567", "#345678", "#456789"] {
            store.save(ColorKind::Accent, color).unwrap();
        }
        drop(store);
        let store = native(&scratch.0).unwrap();
        let saved = store.read().unwrap();
        assert_eq!(saved.accent, ["#234567", "#345678", "#456789"]);
        assert_eq!(saved.background, ["#121212"]);
    }
    #[test]
    fn future_format_is_never_overwritten_by_a_save() {
        let path = std::env::current_dir().unwrap().join("tmp").join(format!(
            "settings-device-future-{}",
            shellfox_core::SessionId::new()
        ));
        std::fs::create_dir(&path).unwrap();
        let scratch = Scratch(path);
        let store = native(&scratch.0).unwrap();
        let future = b"Shellfox device colors 2\naccent=#123456\nbackground=\n";
        std::fs::write(&store.path, future).unwrap();
        assert!(store.read().is_err());
        assert!(store.save(ColorKind::Accent, "#234567").is_err());
        assert_eq!(std::fs::read(&store.path).unwrap(), future);
    }
    #[test]
    fn shared_window_provider_serializes_cross_kind_writes() {
        let path = std::env::current_dir().unwrap().join("tmp").join(format!(
            "settings-device-shared-{}",
            shellfox_core::SessionId::new()
        ));
        std::fs::create_dir(&path).unwrap();
        let scratch = Scratch(path);
        let store = Arc::new(native(&scratch.0).unwrap());
        let other = store.clone();
        let task =
            std::thread::spawn(move || other.save(ColorKind::Background, "#121212").unwrap());
        store.save(ColorKind::Accent, "#123456").unwrap();
        task.join().unwrap();
        let colors = store.read().unwrap();
        assert_eq!(colors.accent, ["#123456"]);
        assert_eq!(colors.background, ["#121212"]);
    }
    #[test]
    fn corrupt_or_future_records_are_not_reset_to_defaults() {
        for bytes in [
            b"bad".as_slice(),
            b"Shellfox device colors 2\naccent=\nbackground=\n",
        ] {
            assert!(decode(Some(bytes)).is_err());
        }
    }
}
