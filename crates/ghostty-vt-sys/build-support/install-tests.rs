use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!(
                "ghostty-vt-build-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, content: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    fn installation(&self) {
        self.write("include/ghostty/vt.h", "/* public umbrella */");
        self.write(
            "include/ghostty/vt/version.h",
            concat!(
                "#define GHOSTTY_VT_VERSION \"1.0.1\"\n",
                "#define GHOSTTY_VT_VERSION_MAJOR 1\n",
                "#define GHOSTTY_VT_VERSION_MINOR 0\n",
                "#define GHOSTTY_VT_VERSION_PATCH 1\n"
            ),
        );
        self.write(
            "lib/pkgconfig/libghostty-vt-static.pc",
            "Version: 1.0.1\nLibs.private: \nRequires.private: \n",
        );
        self.write("lib/pkgconfig/libghostty-vt.pc", "Version: 1.0.1\n");
        self.write("lib/libghostty-vt.a", "static fixture");
        self.write("lib/ghostty-vt.lib", "import fixture");
        self.write("bin/ghostty-vt.dll", "runtime fixture");
    }
    fn validate_static(&self) -> Result<()> {
        validate(
            &self.0,
            &self.0.join("lib"),
            "aarch64-apple-ios",
            "ios",
            "static",
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn pkg_config_fields_are_exact_and_whitespace_tolerant() {
    assert_eq!(pc_field("Version: 1.0.1\r\n", "Version"), Some("1.0.1"));
    assert_eq!(pc_field("Version: 1.0.10\n", "Version"), Some("1.0.10"));
    assert_eq!(pc_field("Libs.private: \n", "Libs.private"), Some(""));
    assert_eq!(pc_field("OtherVersion: 1.0.1", "Version"), None);
}

#[test]
fn exact_prepared_installation_is_accepted() {
    let fixture = Fixture::new();
    fixture.installation();
    fixture.write("ghostty-vt-target.txt", "aarch64-apple-ios\n");
    fixture.validate_static().unwrap();
}

#[test]
fn wrong_header_version_is_rejected() {
    let fixture = Fixture::new();
    fixture.installation();
    fixture.write(
        "include/ghostty/vt/version.h",
        "#define GHOSTTY_VT_VERSION \"1.0.10\"\n",
    );
    assert!(fixture
        .validate_static()
        .unwrap_err()
        .to_string()
        .contains("must declare"));
}

#[test]
fn wrong_library_metadata_version_is_rejected() {
    let fixture = Fixture::new();
    fixture.installation();
    fixture.write("lib/pkgconfig/libghostty-vt-static.pc", "Version: 1.0.0\n");
    assert!(fixture
        .validate_static()
        .unwrap_err()
        .to_string()
        .contains("Version: 1.0.1"));
}

#[test]
fn wrong_prepared_target_is_rejected() {
    let fixture = Fixture::new();
    fixture.installation();
    fixture.write("ghostty-vt-target.txt", "aarch64-apple-ios-sim\n");
    assert!(fixture
        .validate_static()
        .unwrap_err()
        .to_string()
        .contains("another Rust target"));
}

#[test]
fn unhandled_external_static_dependencies_are_rejected() {
    let fixture = Fixture::new();
    fixture.installation();
    for dependency in [
        "Libs.private: -lc++",
        "Requires.private: libhwy",
        "Requires: simdutf",
    ] {
        fixture.write(
            "lib/pkgconfig/libghostty-vt-static.pc",
            &format!("Version: 1.0.1\n{dependency}\n"),
        );
        assert!(fixture
            .validate_static()
            .unwrap_err()
            .to_string()
            .contains("external dependencies"));
    }
}

#[test]
fn windows_requires_dll_as_well_as_import_library() {
    let fixture = Fixture::new();
    fixture.installation();
    validate(
        &fixture.0,
        &fixture.0.join("lib"),
        "x86_64-pc-windows-msvc",
        "windows",
        "dylib",
    )
    .unwrap();
    fs::remove_file(fixture.0.join("bin/ghostty-vt.dll")).unwrap();
    assert!(validate(
        &fixture.0,
        &fixture.0.join("lib"),
        "x86_64-pc-windows-msvc",
        "windows",
        "dylib"
    )
    .is_err());
}

#[test]
fn overridden_library_directory_is_version_checked() {
    let fixture = Fixture::new();
    fixture.installation();
    fixture.write(
        "lib64/pkgconfig/libghostty-vt-static.pc",
        "Version: 1.0.1\n",
    );
    fixture.write("lib64/libghostty-vt.a", "static fixture");
    validate(
        &fixture.0,
        &fixture.0.join("lib64"),
        "aarch64-apple-ios",
        "ios",
        "static",
    )
    .unwrap();
}

#[test]
fn invalid_archive_is_rejected_before_extraction() {
    let fixture = Fixture::new();
    fixture.write(
        &format!("build-support/vendor/{WINDOWS_ARCHIVE}"),
        "not a zip",
    );
    let error = windows_archive(&fixture.0, &fixture.0.join("out")).unwrap_err();
    assert!(error.to_string().contains("SHA-256 mismatch"));
    assert!(!fixture.0.join("out").exists());
}

#[test]
fn runtime_staging_supports_custom_profiles_and_cross_target_directories() {
    let fixture = Fixture::new();
    fixture.installation();
    fixture.write("notices/LICENSE", "license fixture");
    let profile = fixture
        .0
        .join("target/x86_64-pc-windows-msvc/custom-profile");
    let out = profile.join("build/sys-hash/out");
    fs::create_dir_all(&out).unwrap();
    stage_windows_runtime(&fixture.0, &out, &fixture.0.join("notices")).unwrap();
    // A second run must not rewrite an unchanged DLL, including one loaded by Windows.
    stage_windows_runtime(&fixture.0, &out, &fixture.0.join("notices")).unwrap();
    for dir in [profile.clone(), profile.join("deps")] {
        assert_eq!(
            fs::read_to_string(dir.join("ghostty-vt.dll")).unwrap(),
            "runtime fixture"
        );
        assert_eq!(
            fs::read_to_string(dir.join("ghostty-vt-notices/LICENSE")).unwrap(),
            "license fixture"
        );
    }
}

#[test]
fn unexpected_output_layout_is_rejected() {
    let fixture = Fixture::new();
    fixture.installation();
    assert!(stage_windows_runtime(&fixture.0, &fixture.0.join("wrong/out"), &fixture.0).is_err());
}
