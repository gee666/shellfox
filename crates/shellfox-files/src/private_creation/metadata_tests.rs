use super::*;
use std::io::Write;
#[test]
fn bounded_private_read_keeps_full_identity_and_never_repairs_an_existing_file() {
    let f=Fixture::new();
    let value=create_private_file_at(f.parent(),OsStr::new("prefs"),PrivateFileUse::Stage).unwrap();
    let mut file=value.as_file();file.write_all(b"private metadata").unwrap();file.sync_all().unwrap();
    let before=state(&f.path.join("prefs"));
    let read=bounded_private_read_at(f.parent(),OsStr::new("prefs"),100).unwrap();
    assert_eq!(read.bytes,b"private metadata");
    assert_eq!(read.version.identity(),value.identity());
    assert_eq!(read.version.size(),16);
    assert_eq!(read.version.revision().len(),64);
    assert!(bounded_private_read_at(f.parent(),OsStr::new("prefs"),3).is_err());
    assert!(bounded_private_read_at(f.parent(),OsStr::new("prefs"),PRIVATE_METADATA_MAX_BYTES+1).is_err());
    assert_eq!(state(&f.path.join("prefs")),before);
    assert_eq!(fs::read(f.path.join("prefs")).unwrap(),b"private metadata");
}
