/// Preserve the whole path for roots. Else use the final directory name.
/// The path can come from a host running a different OS than the client.
pub fn directory_session_title(cwd: &str) -> &str {
    let drive = matches!(cwd.as_bytes(), [letter, b':', b'/' | b'\\', ..] if letter.is_ascii_alphabetic());
    let unc = cwd.starts_with(r"\\");
    if drive || unc {
        let tail = cwd.trim_end_matches(['/', '\\']);
        let share_root = tail
            .strip_prefix(r"\\")
            .is_some_and(|prefix| prefix.split(['/', '\\']).count() == 2);
        if drive && tail.len() == 2 || share_root {
            return cwd;
        }
        return tail.rsplit(['/', '\\']).find(|part| !part.is_empty()).unwrap_or(cwd);
    }
    cwd.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gives_roots_the_full_path_and_other_directories_their_name() {
        for (cwd, title) in [
            ("/", "/"),
            (r"C:\", r"C:\"),
            ("P:/", "P:/"),
            (r"\\host\share\", r"\\host\share\"),
            (r"\\wsl.localhost\Debian\tmp", "tmp"),
            (r"C:\src\project\", "project"),
            (r"/home/test/back\slash/", r"back\slash"),
        ] {
            assert_eq!(directory_session_title(cwd), title);
        }
    }
}
