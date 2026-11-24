use super::super::*;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub struct Environment {
    pub platform: SystemPlatform,
    pub home: PathBuf,
    pub variables: BTreeMap<String, OsString>,
}
impl SystemEnvironment for Environment {
    fn platform(&self) -> SystemPlatform {
        self.platform
    }
    fn home(&self) -> SystemResult<PathBuf> {
        Ok(self.home.clone())
    }
    fn variable(&self, name: &str) -> Option<OsString> {
        self.variables
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
    }
}
#[derive(Default)]
pub struct Opener(pub Mutex<Vec<SystemOpenTarget>>);
impl SystemOpener for Opener {
    fn open(&self, target: SystemOpenTarget) -> SystemResult<()> {
        self.0.lock().unwrap().push(target);
        Ok(())
    }
}
pub struct Discovery(pub LinuxMenuDiscovery);
impl SystemLinuxDiscovery for Discovery {
    fn inspect(&self) -> SystemResult<LinuxMenuDiscovery> {
        Ok(self.0.clone())
    }
}

pub struct Fixture {
    pub _temp: tempfile::TempDir,
    pub root: PathBuf,
    pub plan: CliInstallPlan,
    pub resources: Arc<ConfiguredSystemResources>,
    pub env: Arc<Environment>,
    pub opener: Arc<Opener>,
    pub files: Arc<NativeSystemFiles>,
}
impl Fixture {
    pub fn new(platform: SystemPlatform) -> Self {
        let project = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let scratch = project.join("tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("system-owned-")
            .tempdir_in(scratch)
            .unwrap();
        let root = temp.path().to_owned();
        let package = root.join("package");
        std::fs::create_dir(&package).unwrap();
        let exe = package.join("Shellfox.exe");
        std::fs::write(&exe, b"synthetic package executable; never run").unwrap();
        let directory = root.join("bin");
        let files = vec![ManagedFile {
            path: directory.join("shellfox"),
            bytes: b"#!/bin/sh\n# Shellfox/linux-v1\n# synthetic; never executed\n".to_vec(),
            executable: true,
        }];
        let marker = files.last().unwrap().path.clone();
        let plan = CliInstallPlan {
            directory: directory.clone(),
            files,
            marker,
            package_managed: false,
        };
        let desktop = DesktopLaunch {
            executable: exe,
            prefix_arguments: Vec::new(),
            new_session_contract: true,
            disposition: DesktopLaunchDisposition::ExistingWindow,
            cli_start: Some(directory.join("shellfox")),
        };
        let resources = Arc::new(
            ConfiguredSystemResources::fixture(package, plan.clone(), Some(desktop)).unwrap(),
        );
        let env = Arc::new(Environment {
            platform,
            home: root.clone(),
            variables: [
                ("PATH".into(), directory.clone().into_os_string()),
                ("XDG_DATA_HOME".into(), root.join("data").into_os_string()),
                (
                    "XDG_CONFIG_HOME".into(),
                    root.join("config").into_os_string(),
                ),
            ]
            .into_iter()
            .collect(),
        });
        Self {
            _temp: temp,
            root,
            plan,
            resources,
            env,
            opener: Arc::new(Opener::default()),
            files: Arc::new(NativeSystemFiles),
        }
    }
    pub fn backend(
        &self,
        managers: Vec<FileManager>,
        policy: SystemInstallPolicy,
    ) -> Arc<NativeSystemBackend> {
        Arc::new(NativeSystemBackend::new(
            SystemProviders {
                environment: self.env.clone(),
                resources: self.resources.clone(),
                files: self.files.clone(),
                opener: self.opener.clone(),
                linux_discovery: Arc::new(Discovery(LinuxMenuDiscovery {
                    managers,
                    nautilus_python: Some(false),
                })),
            },
            policy,
        ))
    }
    pub fn services(&self) -> HostSystemServices {
        HostSystemServices::new(self.backend(
            Vec::new(),
            SystemInstallPolicy {
                allow_user_install: true,
                allow_open: true,
            },
        ))
    }
    pub fn put_cli(&self) {
        for file in &self.plan.files {
            self.files
                .replace(&file.path, None, Some(&file.bytes), file.executable)
                .unwrap();
        }
    }
}
