//! Canonical browser names for a full-account file service.
//! All subsequent opens and publication still use the no-follow file service.
use super::*;

impl FileService {
    pub fn browser_existing(&self, input: &Location) -> Result<Location> {
        if self.root.is_some() {
            return Ok(input.clone());
        }
        self.authorized(false)?;
        let path = PathBuf::from(&input.path);
        paths::validate_absolute_syntax(&path)?;
        let resolved = browser_canonical(&path)?;
        self.authorized(false)?;
        canonical_location(&resolved)
    }

    pub fn browser_create_target(&self, input: &Location) -> Result<Location> {
        if self.root.is_some() {
            return Ok(input.clone());
        }
        self.authorized(true)?;
        let path = PathBuf::from(&input.path);
        paths::validate_absolute_syntax(&path)?;
        let mut missing = Vec::new();
        let mut check = path.as_path();
        let mut resolved = loop {
            match browser_canonical(check) {
                Ok(path) => break path,
                Err(error) if error.code == ErrorCode::NotFound => {
                    let Some(name) = check.file_name() else {
                        return Err(error);
                    };
                    let Some(parent) = check.parent() else {
                        return Err(error);
                    };
                    missing.push(name);
                    check = parent;
                }
                Err(error) => return Err(error),
            }
        };
        for name in missing.into_iter().rev() {
            resolved.push(name);
        }
        self.authorized(true)?;
        canonical_location(&resolved)
    }
}

fn canonical_location(native: &Path) -> Result<Location> {
    paths::validate_absolute_syntax(native)?;
    Ok(Location {
        path: paths::display(native)?,
    })
}

fn browser_canonical(path: &Path) -> Result<PathBuf> {
    let result = std::fs::canonicalize(path)?;
    // Windows canonicalize returns extended paths. Never allow an unrelated UNC
    // server, device, or alternate stream to acquire authority through a link.
    #[cfg(windows)]
    let result = {
        use std::path::{Component, Prefix};
        let Some(Component::Prefix(prefix)) = result.components().next() else {
            return Err(FileError::denied());
        };
        match prefix.kind() {
            Prefix::Disk(_) => result,
            Prefix::VerbatimDisk(_) => {
                let text = result.to_str().ok_or_else(FileError::denied)?;
                PathBuf::from(text.strip_prefix(r"\\?\").ok_or_else(FileError::denied)?)
            }
            _ => return Err(FileError::denied()),
        }
    };
    canonical_location(&result)?;
    Ok(result)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn only_full_account_browser_paths_follow_symlinks_and_keep_canonical_names() {
        let tmp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let tmp = tmp.canonicalize().unwrap();
        let scratch = tmp.join(format!("browser-link-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&scratch).unwrap();
        let folder = scratch.join("directory");
        let actual = folder.join("note.txt");
        let alias = scratch.join("alias");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(&actual, "text").unwrap();
        std::os::unix::fs::symlink(&folder, &alias).unwrap();
        let input = Location {
            path: alias.join("note.txt").to_str().unwrap().to_owned(),
        };
        let scoped = FileService::approved_root(&scratch, Access::ReadWrite).unwrap();
        assert_eq!(scoped.browser_existing(&input).unwrap().path, input.path);
        assert!(scoped.read_text(&input).is_err());
        let service = FileService::local_desktop(Access::ReadWrite);
        let resolved = service.browser_existing(&input).unwrap();
        assert_eq!(Path::new(&resolved.path), actual);
        assert_eq!(service.read_text(&resolved).unwrap().text, "text");
        let nested = Location {
            path: alias.join("one/two").to_str().unwrap().to_owned(),
        };
        let create = service.browser_create_target(&nested).unwrap();
        assert_eq!(Path::new(&create.path), folder.join("one/two"));
        assert!(!folder.join("one").exists());
        drop(scoped);
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
