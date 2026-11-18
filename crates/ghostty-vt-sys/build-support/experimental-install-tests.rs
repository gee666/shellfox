use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture {
    root: PathBuf,
    prefix: PathBuf,
    out: PathBuf,
    project: PathBuf,
    profile: String,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let project = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = project.join("tmp").join(format!(
            "native002-install-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let prefix = root.join("install");
        let out = root.join("target/debug/build/sys/out");
        fs::create_dir_all(&out).unwrap();
        let mut recipe = FIXED
            .iter()
            .map(|(key, value)| format!("{key} {value}\n"))
            .collect::<String>();
        recipe.push_str("rust-target x86_64-pc-windows-msvc\nzig-target x86_64-windows-gnu\n");
        let profile = format!("{:x}", Sha256::digest(recipe.as_bytes()));
        fs::create_dir_all(&prefix).unwrap();
        fs::write(prefix.join(RECIPE), recipe).unwrap();
        let support = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("build-support");
        let patches = [
            support.join("patches/ansi-export/checkpoints/native-002/0001-ansi-export.patch"),
            support.join(
                "patches/terminal-profile/checkpoints/native-002/0002-terminal-profile.patch",
            ),
            support
                .join("patches/snapshot-budget/checkpoints/native-002/0003-snapshot-budget.patch"),
        ];
        let mut manifest = String::new();
        for &(key, expected) in FIXED.iter().filter(|(key, _)| key.starts_with("header-")) {
            let path = format!("include/ghostty/vt/{}", &key[7..]);
            let marker = format!("+++ b/{path}");
            let patch = patches
                .iter()
                .map(|p| fs::read_to_string(p).unwrap())
                .find(|p| p.lines().any(|l| l == marker))
                .unwrap();
            let section = patch.split_once(&marker).unwrap().1;
            let mut header = String::new();
            for line in section.lines().skip(1) {
                if line.starts_with("--- ") {
                    break;
                }
                if let Some(text) = line.strip_prefix('+') {
                    header.push_str(text);
                    header.push('\n');
                }
            }
            assert_eq!(format!("{:x}", Sha256::digest(header.as_bytes())), expected);
            let file = prefix.join(&path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, header).unwrap();
            manifest.push_str(&format!("{expected}  {path}\n"));
        }
        for path in [
            "lib/pkgconfig/libghostty-vt.pc",
            "lib/pkgconfig/libghostty-vt-static.pc",
            "lib/ghostty-vt.lib",
            "bin/ghostty-vt.dll",
        ] {
            let file = prefix.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, b"fixture, not executable").unwrap();
            manifest.push_str(&format!("{}  {path}\n", digest(&file).unwrap()));
        }
        fs::write(prefix.join(FILES), manifest).unwrap();
        Self {
            root,
            prefix,
            out,
            project,
            profile,
        }
    }
    fn validate(&self) -> Result<()> {
        validate(
            &self.prefix,
            &self.prefix.join("lib"),
            "x86_64-pc-windows-msvc",
            "windows",
            &self.profile,
            &self.project,
            &self.out,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn recipe_fields_refuse_duplicates_crlf_and_empty_values() {
    assert!(fields("a b\na c\n").is_err());
    assert!(fields("a b\r\n").is_err());
    assert!(fields("a \n").is_err());
    assert_eq!(fields("a b\n").unwrap().get("a"), Some(&"b"));
}
#[test]
fn profile_spelling_is_exact() {
    assert!(sha(&"a".repeat(64)));
    assert!(!sha(&"A".repeat(64)));
    assert!(!sha(&"a".repeat(63)));
}
#[test]
fn exact_prepared_candidate_is_accepted() {
    assert!(Fixture::new().validate().is_ok());
}
#[test]
fn changed_header_or_library_is_rejected() {
    for name in [
        "include/ghostty/vt/shellfox_snapshot_budget.h",
        "bin/ghostty-vt.dll",
    ] {
        let fixture = Fixture::new();
        fs::write(fixture.prefix.join(name), b"changed").unwrap();
        assert!(fixture.validate().is_err());
    }
}
#[test]
fn wrong_profile_and_default_output_directory_are_rejected() {
    let fixture = Fixture::new();
    assert!(validate(
        &fixture.prefix,
        &fixture.prefix.join("lib"),
        "x86_64-pc-windows-msvc",
        "windows",
        &"0".repeat(64),
        &fixture.project,
        &fixture.out
    )
    .is_err());
    assert!(validate(
        &fixture.prefix,
        &fixture.prefix.join("lib"),
        "x86_64-pc-windows-msvc",
        "windows",
        &fixture.profile,
        &fixture.project,
        &fixture.project
    )
    .is_err());
}
#[test]
fn changed_patch_pin_is_rejected_even_with_matching_recomputed_profile() {
    let mut fixture = Fixture::new();
    let path = fixture.prefix.join(RECIPE);
    let recipe = fs::read_to_string(&path)
        .unwrap()
        .replace(FIXED[4].1, &"0".repeat(64));
    fixture.profile = format!("{:x}", Sha256::digest(recipe.as_bytes()));
    fs::write(path, recipe).unwrap();
    assert!(fixture.validate().is_err());
}
#[test]
fn incomplete_duplicate_and_unknown_file_records_are_rejected() {
    for change in [0, 1, 2] {
        let fixture = Fixture::new();
        let path = fixture.prefix.join(FILES);
        let value = fs::read_to_string(&path).unwrap();
        let value = match change {
            0 => value.lines().skip(1).collect::<Vec<_>>().join("\n"),
            1 => format!("{value}{}\n", value.lines().next().unwrap()),
            _ => format!("{value}{}  ../outside\n", "0".repeat(64)),
        };
        fs::write(path, value).unwrap();
        assert!(fixture.validate().is_err());
    }
}
#[test]
fn official_installation_without_opt_in_recipe_cannot_pass() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.prefix.join(RECIPE)).unwrap();
    assert!(fixture.validate().is_err());
}
