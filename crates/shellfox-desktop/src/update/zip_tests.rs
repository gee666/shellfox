use super::*;

fn stored_zip(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut directory = Vec::new();
    for &(name, body, mode) in entries {
        let offset = data.len() as u32;
        let size = body.len() as u32;
        let len = name.len() as u16;
        let mut local = [0u8; 30];
        local[..4].copy_from_slice(&0x04034b50u32.to_le_bytes());
        local[18..22].copy_from_slice(&size.to_le_bytes());
        local[22..26].copy_from_slice(&size.to_le_bytes());
        local[26..28].copy_from_slice(&len.to_le_bytes());
        data.extend(local);
        data.extend(name.as_bytes());
        data.extend(body);

        let mut central = [0u8; 46];
        central[..4].copy_from_slice(&0x02014b50u32.to_le_bytes());
        central[20..24].copy_from_slice(&size.to_le_bytes());
        central[24..28].copy_from_slice(&size.to_le_bytes());
        central[28..30].copy_from_slice(&len.to_le_bytes());
        central[38..42].copy_from_slice(&(mode << 16).to_le_bytes());
        central[42..46].copy_from_slice(&offset.to_le_bytes());
        directory.extend(central);
        directory.extend(name.as_bytes());
    }
    let count = entries.len() as u16;
    let mut end = [0u8; 22];
    end[..4].copy_from_slice(&0x06054b50u32.to_le_bytes());
    end[8..10].copy_from_slice(&count.to_le_bytes());
    end[10..12].copy_from_slice(&count.to_le_bytes());
    end[12..16].copy_from_slice(&(directory.len() as u32).to_le_bytes());
    end[16..20].copy_from_slice(&(data.len() as u32).to_le_bytes());
    data.extend(directory);
    data.extend(end);
    data
}

#[test]
fn rejects_archive_links_that_escape_or_have_children() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut entries: Vec<(&str, &[u8], u32)> = vec![
        ("Shellfox.app/Contents/Info.plist", b"plist", 0o100644),
        ("Shellfox.app/Contents/MacOS/Shellfox", b"exec", 0o100755),
        ("Shellfox.app/Contents/Current", b"MacOS", 0o120777),
    ];
    let check = |entries: &[(&str, &[u8], u32)]| {
        std::fs::write(file.path(), stored_zip(entries)).unwrap();
        validate(file.path()).is_ok()
    };
    assert!(check(&entries));
    entries.push(("Shellfox.app/Contents/current/marker", b"payload", 0o100644));
    assert!(!check(&entries));
    entries.pop();
    entries[2].1 = b"../../outside";
    assert!(!check(&entries));
}

#[test]
fn rejects_different_central_and_local_names() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let mut bytes = stored_zip(&[
        ("Shellfox.app/Contents/Info.plist", b"plist", 0o100644),
        ("Shellfox.app/Contents/MacOS/Shellfox", b"exec", 0o100755),
    ]);
    bytes[30] = b'/';
    std::fs::write(file.path(), bytes).unwrap();
    assert!(validate(file.path()).is_err());
}
