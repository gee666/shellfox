#[cfg(windows)]
pub mod junction;
use shellfox_files::{Access, FileService, Location};
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub struct Fixture {
    pub root: PathBuf,
    pub service: FileService,
}
impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}
impl Fixture {
    pub fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("rust-files-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        // canonicalize emits a Windows verbatim path, which is deliberately not accepted.
        let root = std::path::absolute(root).unwrap();
        let root = clean_absolute(&root);
        Self {
            service: FileService::approved_root(&root, Access::ReadWrite).unwrap(),
            root,
        }
    }
    pub fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.root.join(name), bytes).unwrap();
    }
    pub fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.root.join(name)).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // cap-std pins Windows directories against rename/delete while authority is alive.
        let service = std::mem::replace(
            &mut self.service,
            FileService::local_desktop(Access::ReadOnly),
        );
        drop(service);
        fs::remove_dir_all(&self.root).unwrap();
    }
}
pub fn clean_absolute(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        if component == std::path::Component::ParentDir {
            result.pop();
        } else if component != std::path::Component::CurDir {
            result.push(component.as_os_str());
        }
    }
    result
}
pub fn loc(path: &str) -> Location {
    Location {
        path: path.to_owned(),
    }
}
