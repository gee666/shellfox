use shellfox_terminal::wsl::{WslProfileId, WslUploadMapping};
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn mapping_requires_selected_distro_and_native_staging_authority_not_drive_guesses() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tmp")
        .join(format!(
            "wsl-upload-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    std::fs::create_dir_all(root.join("stage")).unwrap();
    let scratch = Scratch(root);
    let file = scratch.0.join("stage/safe file.txt");
    std::fs::write(&file, b"owned upload fixture").unwrap();
    let distro = WslProfileId::parse("wsl:Fixture").unwrap();
    let mapping = WslUploadMapping::new(
        distro.clone(),
        scratch.0.join("stage"),
        "/srv/approved-stage".into(),
    )
    .unwrap();
    assert_eq!(
        mapping.resolve(&distro, &file).unwrap(),
        "/srv/approved-stage/safe file.txt"
    );
    assert!(mapping
        .resolve(&WslProfileId::parse("wsl:Other").unwrap(), &file)
        .is_err());
    let outside = scratch.0.join("outside.txt");
    std::fs::write(&outside, b"not staged").unwrap();
    assert!(mapping.resolve(&distro, &outside).is_err());
    assert!(mapping
        .resolve(&distro, &scratch.0.join("stage/../outside.txt"))
        .is_err());
    let alias = scratch.0.join("stage/alias.txt");
    std::fs::hard_link(&outside, &alias).unwrap();
    assert!(mapping.resolve(&distro, &alias).is_err());
    #[cfg(unix)]
    {
        let link = scratch.0.join("stage/link.txt");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(mapping.resolve(&distro, &link).is_err());
    }
}
