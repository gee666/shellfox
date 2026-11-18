use anyhow::Result;
use shellfox_service::native_providers::NativeProgramResource;
use std::{path::Path, sync::Arc};
fn fixture() -> Result<(tempfile::TempDir, std::path::PathBuf, [u8; 32])> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    let temp = tempfile::Builder::new()
        .prefix("program-lease-")
        .tempdir_in(root)?;
    let file = temp.path().join("declared-input");
    std::fs::write(&file, b"provider-program-fixture\n")?;
    Ok((
        temp,
        file,
        super::inputs::digest("593e72c0b7aabeff5d43bc242b289b5602127a83a1f7404e74045ad61cc542c2")?,
    ))
}
#[test]
fn generic_producer_guards_do_not_claim_immutable_file_validation() -> Result<()> {
    let (_temp, file, digest) = fixture()?;
    let actual = std::fs::File::open(&file)?;
    let resource =
        NativeProgramResource::from_trusted_producer(file.clone(), digest, Arc::new(actual))?;
    std::fs::write(&file, b"Provider-program-fixture\n")?;
    assert!(resource.validate_current().is_err());
    Ok(())
}
#[cfg(windows)]
#[test]
fn factory_windows_lease_blocks_write_delete_and_replacement() -> Result<()> {
    let (temp, file, digest) = fixture()?;
    let resource = NativeProgramResource::from_pinned_file(file.clone(), digest)?;
    assert!(std::fs::OpenOptions::new().write(true).open(&file).is_err());
    assert!(std::fs::remove_file(&file).is_err());
    assert!(std::fs::rename(&file, temp.path().join("moved")).is_err());
    let replacement = temp.path().join("replacement");
    std::fs::write(&replacement, b"other input")?;
    assert!(std::fs::rename(&replacement, &file).is_err());
    resource.validate_current()?;
    drop(resource);
    std::fs::write(&file, b"Provider-program-fixture\n")?;
    assert!(NativeProgramResource::from_pinned_file(file, digest).is_err());
    Ok(())
}
#[cfg(unix)]
#[test]
fn factory_unix_lease_still_rehashes_mutable_file_content() -> Result<()> {
    let (_temp, file, digest) = fixture()?;
    let resource = NativeProgramResource::from_pinned_file(file.clone(), digest)?;
    std::fs::write(&file, b"Provider-program-fixture\n")?;
    assert!(resource.validate_current().is_err());
    Ok(())
}
