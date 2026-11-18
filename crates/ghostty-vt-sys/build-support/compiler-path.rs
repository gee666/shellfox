use std::path::Path;
use std::path::PathBuf;

/// Windows canonicalization returns verbatim paths that NDK clang does not
/// accept in -I arguments. Change only disk/UNC spelling at the compiler edge;
/// keep filesystem validation, native handles and unrelated path policies intact.
pub fn for_compiler(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::{
            ffi::OsString,
            os::windows::ffi::{OsStrExt, OsStringExt},
            path::{Component, Prefix},
        };
        if let Some(Component::Prefix(prefix)) = path.components().next() {
            let units: Vec<u16> = path.as_os_str().encode_wide().collect();
            match prefix.kind() {
                Prefix::VerbatimDisk(_) => return PathBuf::from(OsString::from_wide(&units[4..])),
                Prefix::VerbatimUNC(_, _) => {
                    let mut ordinary = vec![u16::from(b'\\'), u16::from(b'\\')];
                    ordinary.extend_from_slice(&units[8..]);
                    return PathBuf::from(OsString::from_wide(&ordinary));
                }
                _ => {}
            }
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_paths_keep_their_spelling() {
        for value in [
            "include/ghostty",
            "dir with spaces/界",
            "C:\\headers\\ghostty",
        ] {
            assert_eq!(for_compiler(Path::new(value)), PathBuf::from(value));
        }
    }

    #[cfg(windows)]
    #[test]
    fn verbatim_disk_and_unc_keep_unicode_spaces_and_roots() {
        for (input, expected) in [
            (
                r"\\?\P:\build with spaces\界\include",
                r"P:\build with spaces\界\include",
            ),
            (r"\\?\C:\", r"C:\"),
            (
                r"\\?\UNC\server\share\界\include",
                r"\\server\share\界\include",
            ),
        ] {
            assert_eq!(for_compiler(Path::new(input)), PathBuf::from(expected));
        }
    }

    #[cfg(windows)]
    #[test]
    fn device_names_are_not_treated_as_disk_paths() {
        for value in [r"\\.\pipe\test", r"\\?\GLOBALROOT\Device\HarddiskVolume1"] {
            assert_eq!(for_compiler(Path::new(value)), PathBuf::from(value));
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_names_that_look_like_verbatim_paths_are_unchanged() {
        let value = Path::new(r"\\?\C:\include");
        assert_eq!(for_compiler(value), value);
    }
}
