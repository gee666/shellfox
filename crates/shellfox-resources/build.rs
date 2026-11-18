fn main() {
    for variable in [
        "SHELLFOX_WINDOWS_HELPER_MANIFEST_SHA256",
        "SHELLFOX_GO_HELPER_MANIFEST_SHA256",
        "SHELLFOX_DARWIN_HELPERS_MANIFEST_SHA256",
        "SHELLFOX_WSL_SCRIPTS_MANIFEST_SHA256",
    ] {
        println!("cargo:rerun-if-env-changed={variable}");
        let value = match std::env::var(variable) {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => String::new(),
            Err(_) => panic!("{variable} must be Unicode lowercase SHA-256"),
        };
        assert!(
            value.is_empty()
                || (value.len() == 64
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
            "{variable} must be an explicit trusted-build lowercase SHA-256"
        );
        // No bundle directory is inspected here. The build operator supplies
        // this anchor after generating the complete publish manifest.
        println!("cargo:rustc-env={variable}={value}");
    }
}
