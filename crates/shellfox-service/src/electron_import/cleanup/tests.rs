use super::*;
use cap_std::ambient_authority;
use std::fs;

fn link(target: &Path, at: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, at);
    #[cfg(windows)]
    let made = if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, at)
    } else {
        std::os::windows::fs::symlink_file(target, at)
    };
    made.inspect_err(|error| eprintln!("Symlinks unavailable ({error}); skipping link checks."))
        .is_ok()
}

#[test]
fn removal_unlinks_planted_links_without_following_them() {
    let scratch = crate::tests::scratch();
    let root = scratch.path().join("Shellfox");
    let outside = scratch.path().join("outside");
    fs::create_dir_all(root.join("Cache/Cache_Data")).unwrap();
    fs::create_dir_all(outside.join("dir")).unwrap();
    fs::write(outside.join("secret.txt"), b"outside").unwrap();
    fs::write(outside.join("dir/inner.txt"), b"outside").unwrap();
    fs::write(root.join("unknown.bin"), b"keep").unwrap();
    fs::create_dir_all(root.join("rust-v1/host")).unwrap();
    let linked = link(
        &outside.join("secret.txt"),
        &root.join("Cache/Cache_Data/file-link"),
    ) && link(&outside.join("dir"), &root.join("Cache/dir-link"))
        && link(&outside.join("dir"), &root.join("GPUCache"));
    let dir = Dir::open_ambient_dir(&root, ambient_authority()).unwrap();
    remove_caches(&dir);
    assert!(!root.join("Cache").exists());
    if linked {
        assert!(root.join("GPUCache").symlink_metadata().is_err());
    }
    assert_eq!(fs::read(outside.join("secret.txt")).unwrap(), b"outside");
    assert_eq!(fs::read(outside.join("dir/inner.txt")).unwrap(), b"outside");
    assert!(root.join("unknown.bin").is_file() && root.join("rust-v1/host").is_dir());

    // A linked root entry is unlinked, never entered, by the later removal too.
    if link(&outside.join("dir"), &root.join("tailscale")) {
        assert!(remove_electron_data(&dir, &present(&dir)));
        assert!(root.join("tailscale").symlink_metadata().is_err());
        assert!(outside.join("dir/inner.txt").is_file());
    }
    assert!(root.join("unknown.bin").is_file() && root.join("rust-v1/host").is_dir());
}
