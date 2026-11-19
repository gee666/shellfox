use std::{env, fs, path::PathBuf};

pub fn embed() {
    const SOURCE: &str = "SHELLFOX_WSL_FILES_BINARY";
    println!("cargo:rerun-if-env-changed={SOURCE}");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let expression = match env::var_os(SOURCE) {
        None => "None".to_owned(),
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(
                path.is_absolute(),
                "{SOURCE} must name an absolute build artifact"
            );
            println!("cargo:rerun-if-changed={}", path.display());
            let bytes = fs::read(&path).expect("read packaged WSL Files worker");
            let arch = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
                Ok("x86_64") => 62,
                Ok("aarch64") => 183,
                _ => panic!("Unsupported WSL Files worker architecture"),
            };
            assert!(
                bytes.len() <= 32 * 1024 * 1024
                    && bytes.starts_with(b"\x7fELF\x02\x01")
                    && bytes.get(18..20) == Some(&[arch, 0]),
                "{SOURCE} must be a matching Linux ELF"
            );
            fs::write(output.join("shellfox-wsl-files"), bytes).expect("embed WSL Files worker");
            "Some(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/shellfox-wsl-files\")))".to_owned()
        }
    };
    fs::write(output.join("wsl-files-asset.rs"), expression)
        .expect("write WSL Files asset declaration");
}
