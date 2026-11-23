use super::{environment::SystemEnvironment, error, io::SystemFiles, SystemResult};
use shellfox_protocol::ErrorCode;
use std::{io::Read, path::PathBuf, sync::Arc};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileManager {
    Nautilus,
    Nemo,
    Dolphin,
    Thunar,
    Caja,
}
#[derive(Clone, Debug)]
pub struct LinuxMenuDiscovery {
    pub managers: Vec<FileManager>,
    pub nautilus_python: Option<bool>,
}
pub trait SystemLinuxDiscovery: Send + Sync {
    fn inspect(&self) -> SystemResult<LinuxMenuDiscovery>;
}
pub struct NativeSystemLinuxDiscovery {
    pub environment: Arc<dyn SystemEnvironment>,
    pub files: Arc<dyn SystemFiles>,
}
impl SystemLinuxDiscovery for NativeSystemLinuxDiscovery {
    fn inspect(&self) -> SystemResult<LinuxMenuDiscovery> {
        let env = &self.environment;
        let mut directories = Vec::new();
        if let Some(paths) = env.variable("PATH") {
            directories.extend(std::env::split_paths(&paths).filter(|path| path.is_absolute()));
        }
        let data = env
            .variable("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        let data: Vec<_> = std::env::split_paths(&data)
            .filter(|path| path.is_absolute())
            .collect();
        let mut managers = Vec::new();
        for (manager, names) in [
            (FileManager::Nautilus, &["nautilus"][..]),
            (FileManager::Nemo, &["nemo"][..]),
            (FileManager::Dolphin, &["dolphin"][..]),
            (FileManager::Thunar, &["thunar", "Thunar"][..]),
            (FileManager::Caja, &["caja"][..]),
        ] {
            let mut present = false;
            for directory in &directories {
                for name in names {
                    if self.files.program(&directory.join(name))? {
                        present = true;
                    }
                }
            }
            for directory in &data {
                for name in names {
                    if self.files.directory(&directory.join(name))? {
                        present = true;
                    }
                }
            }
            if present {
                managers.push(manager);
            }
        }
        let nautilus_python = if managers.contains(&FileManager::Nautilus) {
            package_status()?
        } else {
            Some(false)
        };
        Ok(LinuxMenuDiscovery {
            managers,
            nautilus_python,
        })
    }
}
fn package_status() -> SystemResult<Option<bool>> {
    let file = match std::fs::File::open(PathBuf::from("/var/lib/dpkg/status")) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(error(
                ErrorCode::NativeUnavailable,
                "Nautilus package status is inaccessible.",
            ))
        }
    };
    let mut text = String::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| {
            error(
                ErrorCode::NativeUnavailable,
                "Nautilus package status could not be read.",
            )
        })?;
    if text.len() > 16 * 1024 * 1024 {
        return Err(error(
            ErrorCode::NativeUnavailable,
            "Package database exceeds the read-only probe bound.",
        ));
    }
    for entry in text.split("\n\n") {
        if entry
            .lines()
            .any(|line| line == "Package: python3-nautilus")
        {
            return Ok(Some(
                entry
                    .lines()
                    .any(|line| line == "Status: install ok installed"),
            ));
        }
    }
    Ok(Some(false))
}
