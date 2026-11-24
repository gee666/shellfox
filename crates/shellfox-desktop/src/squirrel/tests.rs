use super::*;

#[test]
fn path_entry_is_added_once_and_removed_by_normalized_match() {
    let bin = r"C:\Users\A\AppData\Local\shellfox\bin";
    let path = r"C:\Tools;c:\users\a\appdata\local\Shellfox\bin\;D:\x";
    assert_eq!(edit_path(path, bin, false), format!(r"C:\Tools;D:\x;{bin}"));
    assert_eq!(edit_path(path, bin, true), r"C:\Tools;D:\x");
    assert_eq!(edit_path("", bin, false), bin);
}

#[test]
fn explorer_command_matches_electron_verbs() {
    let exe = Path::new(r"C:\Users\A\AppData\Local\shellfox\app-0.10.0\Shellfox.exe");
    assert_eq!(
        explorer_command(exe, false).unwrap(),
        r#""C:\Users\A\AppData\Local\shellfox\app-0.10.0\Shellfox.exe" --new-session --cwd "%1\.""#
    );
    assert!(
        explorer_command(exe, true)
            .unwrap()
            .ends_with(r#"--cwd "%V\.""#)
    );
}

#[test]
fn explorer_takes_over_owned_verbs_and_leaves_foreign_ones() {
    let base = format!(r"Software\Shellfox\Tests\squirrel-{}", std::process::id());
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let keys = ExplorerKeys {
        folder: leak(format!(r"{base}\Folder")),
        background: leak(format!(r"{base}\Background")),
        legacy: [
            leak(format!(r"{base}\LegacyFolder")),
            leak(format!(r"{base}\LegacyBackground")),
        ],
    };
    let user = RegKey::predef(HKEY_CURRENT_USER);
    let cleanup = || {
        let _ = user.delete_subkey_all(&base);
    };
    cleanup();
    let old = Path::new(r"C:\x\app-0.9.15\Shellfox.exe");
    let new = Path::new(r"C:\x\app-0.10.0\Shellfox.exe");
    let result = (|| -> Result<()> {
        explorer(&keys, old, false)?;
        let (legacy, _) = user.create_subkey(keys.legacy[0])?;
        legacy.set_value("PiManagerOwner", &"PiManager/native-v1")?;
        let (foreign, _) = user.create_subkey(keys.legacy[1])?;
        foreign.set_value("PiManagerOwner", &"someone-else")?;
        explorer(&keys, new, false)?;
        assert!(verb_is_current(&user, keys.folder, new, false)?);
        assert!(verb_is_current(&user, keys.background, new, true)?);
        assert!(!verb_is_current(&user, keys.folder, old, false)?);
        // A launch repairs an edited verb and leaves a current one alone.
        user.open_subkey_with_flags(
            format!(r"{}\command", keys.folder),
            winreg::enums::KEY_WRITE,
        )?
        .set_value("", &"edited")?;
        assert!(!verb_is_current(&user, keys.folder, new, false)?);
        explorer(&keys, new, false)?;
        assert!(verb_is_current(&user, keys.folder, new, false)?);
        let command: String = user
            .open_subkey(format!(r"{}\command", keys.folder))?
            .get_value("")?;
        assert_eq!(command, explorer_command(new, false)?);
        assert!(user.open_subkey(keys.legacy[0]).is_err());
        assert!(user.open_subkey(keys.legacy[1]).is_ok());
        user.open_subkey_with_flags(keys.background, winreg::enums::KEY_WRITE)?
            .set_value("ShellfoxOwner", &"other")?;
        explorer(&keys, new, true)?;
        assert!(user.open_subkey(keys.folder).is_err());
        assert!(user.open_subkey(keys.background).is_ok());
        Ok(())
    })();
    cleanup();
    result.unwrap();
}
