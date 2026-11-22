use anyhow::Result;
#[cfg(unix)]
use anyhow::ensure;
#[cfg(target_os = "macos")]
use anyhow::Context;
use semver::Version;
use std::{ffi::OsStr, fs, path::{Path, PathBuf}};
#[cfg(unix)]
use std::{process::{Command, Stdio}, time::{Duration, Instant}};
use tempfile::TempDir;
#[path = "handoff.rs"]
mod handoff;
pub(super) use handoff::Prepared;

#[cfg(any(target_os = "macos", test))]
#[path = "zip.rs"]
mod zip;

#[derive(Clone)]
pub(super) enum Platform {
    #[cfg(windows)]
    Squirrel {
        updater: PathBuf,
    },
    #[cfg(target_os = "linux")]
    Deb {
        executable: PathBuf,
    },
    #[cfg(target_os = "macos")]
    Mac {
        bundle: PathBuf,
    },
    Manual(&'static str),
}
impl Platform {
    pub fn detect() -> Self {
        let Ok(executable) = std::env::current_exe() else {
            return Self::Manual("Could not find the Shellfox executable.");
        };
        if !matches!(std::env::consts::ARCH, "x86_64" | "aarch64") {
            return Self::Manual("No installer is published for this architecture.");
        }
        Self::for_executable(&executable)
    }
    #[cfg(windows)]
    fn for_executable(executable: &Path) -> Self {
        if cfg!(target_arch = "x86_64")
            && executable
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("Shellfox.exe"))
            && let Some(directory) = executable.parent()
            && directory
                .file_name()
                .and_then(OsStr::to_str)
                .and_then(|name| name.strip_prefix("app-"))
                .is_some_and(|version| Version::parse(version).is_ok())
            && let Some(root) = directory.parent()
            && root.join("Update.exe").is_file()
        {
            return Self::Squirrel {
                updater: root.join("Update.exe"),
            };
        }
        Self::Manual("Only x64 installations made with ShellfoxSetup.exe support in-app updates.")
    }
    #[cfg(target_os = "linux")]
    fn for_executable(executable: &Path) -> Self {
        if let Ok(executable) = executable.canonicalize()
            && [
                "/usr/bin/pkexec",
                "/usr/bin/apt-get",
                "/usr/bin/sha256sum",
                "/usr/bin/dpkg-deb",
            ]
            .iter()
            .all(|path| Path::new(path).is_file())
            && capture(
                "/usr/bin/dpkg-query",
                [OsStr::new("-S"), executable.as_os_str()],
            )
            .is_ok_and(|owners| {
                owners.lines().any(|line| {
                    line.split_once(": ").is_some_and(|(owner, path)| {
                        (owner == "shellfox" || owner.strip_prefix("shellfox:") == Some(deb_arch()))
                            && Path::new(path) == executable
                    })
                })
            })
        {
            return Self::Deb { executable };
        }
        Self::Manual("Linux in-app updates require a .deb installation, apt-get and PolicyKit.")
    }
    #[cfg(target_os = "macos")]
    fn for_executable(executable: &Path) -> Self {
        if executable.ends_with("Shellfox.app/Contents/MacOS/Shellfox")
            && let Some(bundle) = executable.ancestors().nth(3)
            && bundle.canonicalize().is_ok_and(|path| path == bundle)
            && bundle_value(bundle, "CFBundleIdentifier").is_ok_and(|id| id == "local.shellfox")
            && bundle.parent().is_some_and(|parent| {
                Command::new("/bin/test")
                    .arg("-w")
                    .arg(parent)
                    .status()
                    .is_ok_and(|s| s.success())
            })
        {
            return Self::Mac {
                bundle: bundle.to_owned(),
            };
        }
        Self::Manual("In-app updates require Shellfox.app in a writable Applications folder.")
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    fn for_executable(_: &Path) -> Self {
        Self::Manual("In-app updates are not supported on this platform.")
    }
    pub fn asset_name(&self, version: &Version) -> String {
        if cfg!(windows) {
            "ShellfoxSetup.exe".into()
        } else if cfg!(target_os = "linux") {
            format!("shellfox_{version}_{}.deb", deb_arch())
        } else {
            format!(
                "Shellfox-darwin-{}-{version}.zip",
                if cfg!(target_arch = "aarch64") {
                    "arm64"
                } else {
                    "x64"
                }
            )
        }
    }
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Manual(reason) => Some(reason),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    pub fn prepare(
        &self,
        file: PathBuf,
        _version: &Version,
        work: TempDir,
        asset: &super::feed::Asset,
    ) -> Result<Prepared> {
        let sha256 = asset.sha256.clone();
        #[cfg(target_os = "macos")]
        let mut stage = None;
        let script = match self {
            #[cfg(windows)]
            Self::Squirrel { .. } => {
                include_str!("install-windows.ps1")
            }
            #[cfg(target_os = "linux")]
            Self::Deb { .. } => {
                for (field, expected) in [
                    ("Package", "shellfox".to_owned()),
                    ("Version", deb_version(_version)),
                    ("Architecture", deb_arch().into()),
                ] {
                    let actual = capture(
                        "/usr/bin/dpkg-deb",
                        [OsStr::new("-f"), file.as_os_str(), OsStr::new(field)],
                    )?;
                    ensure!(actual == expected, "Unexpected Debian package {field}");
                }
                include_str!("install-linux.sh")
            }
            #[cfg(target_os = "macos")]
            Self::Mac { bundle } => {
                stage = Some(stage_mac(&file, _version, work.path(), bundle)?);
                include_str!("install-mac.sh")
            }
            Self::Manual(reason) => anyhow::bail!("{reason}"),
        };
        let helper = work.path().join(if cfg!(windows) {
            "install.ps1"
        } else {
            "install.sh"
        });
        fs::write(&helper, script)?;
        Ok(Prepared {
            platform: self.clone(),
            work,
            helper,
            file,
            sha256,
            size: asset.size,
            #[cfg(target_os = "macos")]
            stage,
        })
    }
}
/// Same mapping as scripts/release-version.mjs: test builds use "~" so they
/// sort below their final release.
#[cfg(any(target_os = "linux", test))]
fn deb_version(version: &Version) -> String {
    let core = format!("{}.{}.{}", version.major, version.minor, version.patch);
    if version.pre.is_empty() {
        core
    } else {
        format!("{core}~{}", version.pre)
    }
}
#[cfg(test)]
#[test]
fn deb_versions_match_the_package_mapping() {
    assert_eq!(deb_version(&Version::new(0, 10, 0)), "0.10.0");
    assert_eq!(deb_version(&Version::parse("0.10.0-beta.1").unwrap()), "0.10.0~beta.1");
}
fn deb_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "amd64"
    }
}

#[cfg(unix)]
fn capture(executable: &str, args: impl IntoIterator<Item: AsRef<OsStr>>) -> Result<String> {
    // Use files rather than pipes so unexpected diagnostic output cannot block the child.
    let mut output = tempfile::tempfile()?;
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Update verification timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    ensure!(
        status.success(),
        "Update verification command failed: {executable}"
    );
    use std::io::{Read, Seek};
    output.rewind()?;
    let mut text = String::new();
    output.take(1024 * 1024 + 1).read_to_string(&mut text)?;
    ensure!(
        text.len() <= 1024 * 1024,
        "Update verification output is too large"
    );
    Ok(text.trim().to_owned())
}

#[cfg(target_os = "macos")]
fn bundle_value(bundle: &Path, key: &str) -> Result<String> {
    capture(
        "/usr/libexec/PlistBuddy",
        [
            OsStr::new("-c"),
            OsStr::new(&format!("Print :{key}")),
            bundle.join("Contents/Info.plist").as_os_str(),
        ],
    )
}
#[cfg(target_os = "macos")]
fn stage_mac(file: &Path, version: &Version, work: &Path, bundle: &Path) -> Result<TempDir> {
    zip::validate(file)?;
    let extracted = work.join("extracted");
    capture(
        "/usr/bin/ditto",
        [
            OsStr::new("-x"),
            OsStr::new("-k"),
            file.as_os_str(),
            extracted.as_os_str(),
        ],
    )?;
    let app = extracted.join("Shellfox.app");
    ensure!(
        bundle_value(&app, "CFBundleIdentifier")? == "local.shellfox"
            && bundle_value(&app, "CFBundleShortVersionString")? == version.to_string(),
        "Unexpected app bundle"
    );
    let verify = |app: &Path| {
        capture(
            "/usr/bin/codesign",
            [
                OsStr::new("--verify"),
                OsStr::new("--deep"),
                OsStr::new("--strict"),
                app.as_os_str(),
            ],
        )
    };
    verify(&app)?;
    let stage = tempfile::Builder::new()
        .prefix(".Shellfox-update-")
        .tempdir_in(bundle.parent().context("Missing app parent")?)?;
    let incoming = stage.path().join("Shellfox.app");
    capture("/usr/bin/ditto", [app.as_os_str(), incoming.as_os_str()])?;
    verify(&incoming)?;
    Ok(stage)
}
